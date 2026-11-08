use std::time::Instant;

use wasmi::{
    Caller, CompilationMode, Config, Engine, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

use crate::{Runtime, ms, rss_kib, time};

pub struct State {
    limits: StoreLimits,
    ext_rule: Option<TypedFunc<u32, u32>>,
    cross: bool,
}

pub struct Wasmi {
    mode: CompilationMode,
    fuel: bool,
    store: Store<State>,
    a: Instance,
    b: Instance,
    mem_a: Memory,
    doc_ptr: u32,
    doc_len: u32,
}

fn engine(mode: CompilationMode, fuel: bool) -> Engine {
    let mut c = Config::default();
    c.compilation_mode(mode);
    c.consume_fuel(fuel);
    Engine::new(&c)
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

impl Wasmi {
    pub fn new(wasm: &[u8], mode: CompilationMode, fuel: bool) -> Self {
        let engine = engine(mode, fuel);
        let module = Module::new(&engine, wasm).unwrap();
        let linker = linker(&engine);
        let mut store = new_store(&engine, StoreLimitsBuilder::new().build());
        if fuel {
            store.set_fuel(u64::MAX / 2).unwrap();
        }
        let a = linker.instantiate_and_start(&mut store, &module).unwrap();
        let b = linker.instantiate_and_start(&mut store, &module).unwrap();
        let rule = b.get_typed_func::<u32, u32>(&store, "rule").unwrap();
        store.data_mut().ext_rule = Some(rule);
        let mem_a = a.get_memory(&store, "memory").unwrap();
        Wasmi {
            mode,
            fuel,
            store,
            a,
            b,
            mem_a,
            doc_ptr: 0,
            doc_len: 0,
        }
    }

    fn write(&mut self, inst: Instance, doc: &str) -> u32 {
        let alloc = inst
            .get_typed_func::<u32, u32>(&self.store, "alloc")
            .unwrap();
        let p = alloc.call(&mut self.store, doc.len() as u32).unwrap();
        let mem = inst.get_memory(&self.store, "memory").unwrap();
        mem.write(&mut self.store, p as usize, doc.as_bytes())
            .unwrap();
        p
    }

    pub fn fuel_left(&self) -> u64 {
        self.store.get_fuel().unwrap()
    }
}

impl Runtime for Wasmi {
    fn name(&self) -> String {
        format!(
            "wasmi {:?}{}",
            self.mode,
            if self.fuel { " +fuel" } else { "" }
        )
    }
    fn load_doc(&mut self, doc: &str) {
        let _ = self.mem_a;
        self.doc_ptr = self.write(self.a, doc);
        self.doc_len = doc.len() as u32;
        let pb = self.write(self.b, doc);
        let set_doc = self
            .b
            .get_typed_func::<(u32, u32), ()>(&self.store, "set_doc")
            .unwrap();
        set_doc
            .call(&mut self.store, (pb, doc.len() as u32))
            .unwrap();
    }
    fn parse(&mut self, start: usize, end: usize) -> u32 {
        let f = self
            .a
            .get_typed_func::<(u32, u32), u32>(&self.store, "parse")
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
            .get_typed_func::<(u32, u32), u64>(&self.store, name)
            .unwrap();
        let r = f
            .call(&mut self.store, (self.doc_ptr, self.doc_len))
            .unwrap();
        (r >> 32, r & 0xffff_ffff)
    }
}

pub fn startup(wasm: &[u8], mode: CompilationMode) {
    let r0 = rss_kib();
    let t = Instant::now();
    let engine = engine(mode, false);
    let module = Module::new(&engine, wasm).unwrap();
    let t_compile = t.elapsed();
    let linker = linker(&engine);
    let mut store = new_store(&engine, StoreLimitsBuilder::new().build());
    let t = Instant::now();
    let a = linker.instantiate_and_start(&mut store, &module).unwrap();
    let t_inst = t.elapsed();
    // 最初の呼び出し（遅延翻訳の場合はここで翻訳される）
    let f = a
        .get_typed_func::<(u32, u32), u32>(&store, "parse")
        .unwrap();
    let alloc = a.get_typed_func::<u32, u32>(&store, "alloc").unwrap();
    let src = "# a\n\nb *c* [[d]]\n";
    let p = alloc.call(&mut store, src.len() as u32).unwrap();
    a.get_memory(&store, "memory")
        .unwrap()
        .write(&mut store, p as usize, src.as_bytes())
        .unwrap();
    let t = Instant::now();
    f.call(&mut store, (p, src.len() as u32)).unwrap();
    let t_first = t.elapsed();
    println!(
        "{:<26} compile {:>7.2} ms | instantiate {:>6.3} ms | first small parse {:>6.3} ms | RSS +{} KiB",
        format!("wasmi {mode:?}"),
        ms(t_compile),
        ms(t_inst),
        ms(t_first),
        rss_kib() - r0
    );
}

pub fn limits(wasm: &[u8], doc: &str) {
    // 燃料（命令数による実行時間の上限）
    let mut rt = Wasmi::new(wasm, CompilationMode::Eager, true);
    rt.load_doc(doc);
    let before = rt.fuel_left();
    rt.parse(0, doc.len());
    let used = before - rt.fuel_left();
    let with_fuel = time(10, || {
        rt.parse(0, doc.len());
    });
    println!(
        "wasmi fuel: 1MB parse consumed {used} fuel, parse with fuel med {:.2} ms",
        ms(with_fuel.med)
    );

    let spin = rt.a.get_typed_func::<(), u32>(&rt.store, "spin").unwrap();
    rt.store.set_fuel(1_000_000_000).unwrap();
    let t = Instant::now();
    let r = spin.call(&mut rt.store, ());
    println!(
        "wasmi fuel: spin with 1e9 fuel -> {:?} after {:.1} ms",
        r.map_err(|e| e.to_string()),
        ms(t.elapsed())
    );
    rt.store.set_fuel(u64::MAX / 2).unwrap();
    let after = rt.parse(0, doc.find("\n\n").unwrap());
    println!("wasmi fuel: instance usable after trap: parse -> {after} events");

    // メモリの上限
    let engine = engine(CompilationMode::Eager, false);
    let module = Module::new(&engine, wasm).unwrap();
    let mut store = new_store(
        &engine,
        StoreLimitsBuilder::new().memory_size(64 << 20).build(),
    );
    let inst = linker(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    let mem = inst.get_memory(&store, "memory").unwrap();
    let initial = mem.size(&store);
    let grow = inst.get_typed_func::<(), u32>(&store, "grow").unwrap();
    let n = grow.call(&mut store, ()).unwrap();
    println!(
        "wasmi memory limit 64 MiB: initial {initial} pages, grew {n} pages, final {} pages ({} MiB)",
        mem.size(&store),
        mem.size(&store) * 64 / 1024
    );
}
