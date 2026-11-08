use std::time::Instant;

use wasmtime::{
    Caller, Config, Engine, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

use crate::{Runtime, ms, rss_kib, time};

pub struct State {
    limits: StoreLimits,
    ext_rule: Option<TypedFunc<u32, u32>>,
    cross: bool,
}

pub struct Wasmtime {
    label: String,
    store: Store<State>,
    a: Instance,
    b: Instance,
    doc_ptr: u32,
    doc_len: u32,
}

#[derive(Clone, Copy, Default)]
pub struct Opts {
    pub pulley: bool,
    pub fuel: bool,
    pub epoch: bool,
}

pub fn engine(o: Opts) -> Engine {
    let mut c = Config::new();
    if o.pulley {
        c.target("pulley64").unwrap();
    }
    c.consume_fuel(o.fuel);
    c.epoch_interruption(o.epoch);
    Engine::new(&c).unwrap()
}

fn linker(engine: &Engine) -> Linker<State> {
    let mut l = Linker::<State>::new(engine);
    l.func_wrap(
        "host",
        "call_rule",
        |mut caller: Caller<'_, State>, pos: u32| -> u32 {
            if !caller.data().cross {
                return 0;
            }
            let f = caller.data().ext_rule.clone().unwrap();
            f.call(&mut caller, pos).unwrap()
        },
    )
    .unwrap();
    l
}

fn new_store(engine: &Engine, limits: StoreLimits) -> Store<State> {
    let mut store = Store::new(
        engine,
        State {
            limits,
            ext_rule: None,
            cross: false,
        },
    );
    store.limiter(|s| &mut s.limits);
    store
}

impl Wasmtime {
    fn build(engine: &Engine, module: &Module, label: String, o: Opts) -> Self {
        let linker = linker(engine);
        let mut store = new_store(engine, StoreLimitsBuilder::new().build());
        if o.fuel {
            store.set_fuel(u64::MAX / 2).unwrap();
        }
        if o.epoch {
            store.set_epoch_deadline(u64::MAX / 2);
        }
        let a = linker.instantiate(&mut store, module).unwrap();
        let b = linker.instantiate(&mut store, module).unwrap();
        let rule = b.get_typed_func::<u32, u32>(&mut store, "rule").unwrap();
        store.data_mut().ext_rule = Some(rule);
        Wasmtime {
            label,
            store,
            a,
            b,
            doc_ptr: 0,
            doc_len: 0,
        }
    }

    #[cfg(feature = "wasmtime-cranelift")]
    pub fn from_wasm(wasm: &[u8], pulley: bool, fuel: bool) -> Self {
        Self::with_opts(
            wasm,
            Opts {
                pulley,
                fuel,
                epoch: false,
            },
        )
    }

    #[cfg(feature = "wasmtime-cranelift")]
    pub fn with_opts(wasm: &[u8], o: Opts) -> Self {
        let engine = engine(o);
        let module = Module::new(&engine, wasm).unwrap();
        let label = format!(
            "wasmtime {}{}{}",
            if o.pulley { "pulley" } else { "cranelift" },
            if o.fuel { " +fuel" } else { "" },
            if o.epoch { " +epoch" } else { "" }
        );
        Self::build(&engine, &module, label, o)
    }

    pub fn from_precompiled(cwasm: &[u8], fuel: bool) -> Self {
        let o = Opts {
            pulley: true,
            fuel,
            epoch: false,
        };
        let engine = engine(o);
        // 自分で生成したファイルのみを読み込む（deserializeは信頼できる入力を前提とする）。
        let module = unsafe { Module::deserialize(&engine, cwasm) }.unwrap();
        Self::build(&engine, &module, "wasmtime pulley (precompiled)".into(), o)
    }

    fn write(&mut self, inst: Instance, doc: &str) -> u32 {
        let alloc = inst
            .get_typed_func::<u32, u32>(&mut self.store, "alloc")
            .unwrap();
        let p = alloc.call(&mut self.store, doc.len() as u32).unwrap();
        let mem = inst.get_memory(&mut self.store, "memory").unwrap();
        mem.write(&mut self.store, p as usize, doc.as_bytes())
            .unwrap();
        p
    }
}

impl Runtime for Wasmtime {
    fn name(&self) -> String {
        self.label.clone()
    }
    fn load_doc(&mut self, doc: &str) {
        self.doc_ptr = self.write(self.a, doc);
        self.doc_len = doc.len() as u32;
        let pb = self.write(self.b, doc);
        let set_doc = self
            .b
            .get_typed_func::<(u32, u32), ()>(&mut self.store, "set_doc")
            .unwrap();
        set_doc
            .call(&mut self.store, (pb, doc.len() as u32))
            .unwrap();
    }
    fn parse(&mut self, start: usize, end: usize) -> u32 {
        let f = self
            .a
            .get_typed_func::<(u32, u32), u32>(&mut self.store, "parse")
            .unwrap();
        f.call(
            &mut self.store,
            (self.doc_ptr + start as u32, (end - start) as u32),
        )
        .unwrap()
    }
    fn scan(&mut self, mode: u8) -> (u64, u64) {
        self.store.data_mut().cross = mode == 2;
        let name = if mode == 0 { "scan_base" } else { "scan_hooks" };
        let f = self
            .a
            .get_typed_func::<(u32, u32), u64>(&mut self.store, name)
            .unwrap();
        let r = f
            .call(&mut self.store, (self.doc_ptr, self.doc_len))
            .unwrap();
        (r >> 32, r & 0xffff_ffff)
    }
}

fn first_call(engine: &Engine, module: &Module) -> (f64, f64) {
    let linker = linker(engine);
    let mut store = new_store(engine, StoreLimitsBuilder::new().build());
    let t = Instant::now();
    let a = linker.instantiate(&mut store, module).unwrap();
    let t_inst = t.elapsed();
    let f = a
        .get_typed_func::<(u32, u32), u32>(&mut store, "parse")
        .unwrap();
    let alloc = a.get_typed_func::<u32, u32>(&mut store, "alloc").unwrap();
    let src = "# a\n\nb *c* [[d]]\n";
    let p = alloc.call(&mut store, src.len() as u32).unwrap();
    a.get_memory(&mut store, "memory")
        .unwrap()
        .write(&mut store, p as usize, src.as_bytes())
        .unwrap();
    let t = Instant::now();
    f.call(&mut store, (p, src.len() as u32)).unwrap();
    (ms(t_inst), ms(t.elapsed()))
}

#[cfg(feature = "wasmtime-cranelift")]
pub fn startup(wasm: &[u8], pulley: bool) {
    let r0 = rss_kib();
    let t = Instant::now();
    let engine = engine(Opts {
        pulley,
        ..Default::default()
    });
    let module = Module::new(&engine, wasm).unwrap();
    let t_compile = t.elapsed();
    let (inst, first) = first_call(&engine, &module);
    println!(
        "{:<26} compile {:>7.2} ms | instantiate {:>6.3} ms | first small parse {:>6.3} ms | RSS +{} KiB",
        format!("wasmtime {}", if pulley { "pulley" } else { "cranelift" }),
        ms(t_compile),
        inst,
        first,
        rss_kib() - r0
    );
}

pub fn startup_precompiled(cwasm: &[u8]) {
    let r0 = rss_kib();
    let t = Instant::now();
    let engine = engine(Opts {
        pulley: true,
        ..Default::default()
    });
    let module = unsafe { Module::deserialize(&engine, cwasm) }.unwrap();
    let t_load = t.elapsed();
    let (inst, first) = first_call(&engine, &module);
    println!(
        "{:<26} deserialize {:>7.2} ms | instantiate {:>6.3} ms | first small parse {:>6.3} ms | RSS +{} KiB",
        "wasmtime pulley (precomp.)",
        ms(t_load),
        inst,
        first,
        rss_kib() - r0
    );
}

#[cfg(feature = "wasmtime-cranelift")]
pub fn precompile_pulley(wasm: &[u8]) -> Vec<u8> {
    engine(Opts {
        pulley: true,
        ..Default::default()
    })
    .precompile_module(wasm)
    .unwrap()
}

#[cfg(feature = "wasmtime-cranelift")]
pub fn limits(wasm: &[u8], doc: &str, pulley: bool) {
    let kind = if pulley { "pulley" } else { "cranelift" };

    // 燃料
    let mut rt = Wasmtime::with_opts(
        wasm,
        Opts {
            pulley,
            fuel: true,
            epoch: false,
        },
    );
    rt.load_doc(doc);
    let before = rt.store.get_fuel().unwrap();
    rt.parse(0, doc.len());
    let used = before - rt.store.get_fuel().unwrap();
    let with_fuel = time(10, || {
        rt.parse(0, doc.len());
    });
    println!(
        "wasmtime {kind} fuel: 1MB parse consumed {used} fuel, parse with fuel med {:.2} ms",
        ms(with_fuel.med)
    );
    let spin =
        rt.a.get_typed_func::<(), u32>(&mut rt.store, "spin")
            .unwrap();
    rt.store.set_fuel(1_000_000_000).unwrap();
    let t = Instant::now();
    let r = spin.call(&mut rt.store, ());
    println!(
        "wasmtime {kind} fuel: spin with 1e9 fuel -> {:?} after {:.1} ms",
        r.map_err(|e| format!("{:?}", e.downcast_ref::<wasmtime::Trap>())),
        ms(t.elapsed())
    );
    rt.store.set_fuel(u64::MAX / 2).unwrap();
    println!(
        "wasmtime {kind} fuel: instance usable after trap: parse -> {} events",
        rt.parse(0, doc.find("\n\n").unwrap())
    );

    // エポック（壁時計による中断）
    let mut rt = Wasmtime::with_opts(
        wasm,
        Opts {
            pulley,
            fuel: false,
            epoch: true,
        },
    );
    rt.load_doc(doc);
    let with_epoch = time(10, || {
        rt.parse(0, doc.len());
    });
    println!(
        "wasmtime {kind} epoch: parse with epoch checks med {:.2} ms",
        ms(with_epoch.med)
    );
    let ticker_engine = rt.store.engine().clone();
    rt.store.set_epoch_deadline(1);
    let spin =
        rt.a.get_typed_func::<(), u32>(&mut rt.store, "spin")
            .unwrap();
    let ticker = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        ticker_engine.increment_epoch();
    });
    let t = Instant::now();
    let r = spin.call(&mut rt.store, ());
    ticker.join().unwrap();
    println!(
        "wasmtime {kind} epoch: spin interrupted 50 ms after start -> {:?} after {:.1} ms",
        r.map_err(|e| format!("{:?}", e.downcast_ref::<wasmtime::Trap>())),
        ms(t.elapsed())
    );

    // メモリの上限
    let engine = engine(Opts {
        pulley,
        ..Default::default()
    });
    let module = Module::new(&engine, wasm).unwrap();
    let mut store = new_store(
        &engine,
        StoreLimitsBuilder::new().memory_size(64 << 20).build(),
    );
    let inst = linker(&engine).instantiate(&mut store, &module).unwrap();
    let mem = inst.get_memory(&mut store, "memory").unwrap();
    let initial = mem.size(&store);
    let grow = inst.get_typed_func::<(), u32>(&mut store, "grow").unwrap();
    let n = grow.call(&mut store, ()).unwrap();
    println!(
        "wasmtime {kind} memory limit 64 MiB: initial {initial} pages, grew {n} pages, final {} pages ({} MiB)",
        mem.size(&store),
        mem.size(&store) * 64 / 1024
    );
}
