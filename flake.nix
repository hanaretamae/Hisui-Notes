{
  description = "Hisui Notes development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Android 向けの Rust 標準ライブラリを得るために android シェルだけで使う（ADR-0010）。
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = lib.recursiveUpdate
        (forAllSystems (
          pkgs:
          let
            inherit (pkgs) stdenv;
            # GPUI の Linux プラットフォーム層が使うシステムライブラリ（P0-01、ADR-0010）。
            gpuiLinuxLibs = with pkgs; [
              fontconfig
              freetype
              libxkbcommon
              wayland
              vulkan-loader
              libx11
              libxcb
            ];
          in
          {
            default = pkgs.mkShell {
              packages =
                with pkgs;
                [
                  rustc
                  cargo
                  clippy
                  rustfmt
                  rust-analyzer
                  cargo-deny
                  pkg-config
                ]
                ++ lib.optionals stdenv.hostPlatform.isLinux gpuiLinuxLibs;

              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

              # Wayland・Vulkan・X11 のライブラリは実行時に dlopen されるため、探索パスに含める。
              LD_LIBRARY_PATH = lib.optionalString stdenv.hostPlatform.isLinux (
                lib.makeLibraryPath gpuiLinuxLibs
              );
            };
          }
        ))
        {
          # Android 向けのビルドとエミュレーター（P0-05、P0-13）。x86_64 Linux のみ。
          # Android SDK の利用には Android SDK License への同意が必要（android_sdk.accept_license）。
          x86_64-linux.android =
            let
              pkgs = import nixpkgs {
                system = "x86_64-linux";
                overlays = [ rust-overlay.overlays.default ];
                config = {
                  allowUnfree = true;
                  android_sdk.accept_license = true;
                };
              };
              # default シェルの nixpkgs の rustc と同じ版に合わせる。
              rust = pkgs.rust-bin.stable."1.99.0".default.override {
                extensions = [ "rust-src" ];
                targets = [
                  "x86_64-linux-android"
                  "aarch64-linux-android"
                  "wasm32-unknown-unknown"
                ];
              };
              android = pkgs.androidenv.composeAndroidPackages {
                platformVersions = [ "35" ];
                buildToolsVersions = [ "latest" ];
                includeNDK = true;
                ndkVersions = [ "28.2.13676358" ];
                includeEmulator = true;
                includeSystemImages = true;
                systemImageTypes = [ "default" ];
                abiVersions = [ "x86_64" ];
              };
              sdk = "${android.androidsdk}/libexec/android-sdk";
            in
            pkgs.mkShell {
              packages = [
                rust
                android.androidsdk
                pkgs.cargo-ndk
                pkgs.lld
                # APK の作成（apksigner・keytool に JDK が必要）
                pkgs.jdk17_headless
                pkgs.zip
                # GameActivity（AndroidX）を含む APK の作成（P0-05）
                pkgs.gradle_8
              ];
              ANDROID_HOME = sdk;
              ANDROID_SDK_ROOT = sdk;
              ANDROID_NDK_HOME = "${sdk}/ndk/28.2.13676358";
              # avdmanager は XDG_CONFIG_HOME、emulator は $HOME/.android を見るため、置き場所を揃える。
              shellHook = ''
                export ANDROID_USER_HOME="$HOME/.android"
                export ANDROID_AVD_HOME="$ANDROID_USER_HOME/avd"
              '';
            };
        };

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
