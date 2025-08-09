{
  description = "Replicante: Autonomous AI Agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [ "x86_64-unknown-linux-musl" ];
        };
      in
      {
        # Default package: standard build
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "replicante";
          version = "0.1.0";
          src = ./.;
          
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          
          nativeBuildInputs = with pkgs; [
            pkg-config
            rustToolchain
          ];
          
          buildInputs = with pkgs; [
            sqlite
            openssl
          ];
          
          meta = with pkgs.lib; {
            description = "Autonomous AI Agent";
            license = licenses.mit;
          };
        };
        
        # Static musl build - following cyberkrill pattern exactly
        packages.replicante-static = let
          rustPlatformMusl = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in rustPlatformMusl.buildRustPackage {
          pname = "replicante-static";
          version = "0.1.0";
          src = ./.;
          
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          
          nativeBuildInputs = with pkgs; [
            pkg-config
            rustToolchain
            pkgsStatic.stdenv.cc
          ];
          
          buildInputs = with pkgs.pkgsStatic; [
            sqlite
          ];
          
          # Force cargo to use the musl target
          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsStatic.stdenv.cc}/bin/${pkgs.pkgsStatic.stdenv.cc.targetPrefix}cc";
          CC_x86_64_unknown_linux_musl = "${pkgs.pkgsStatic.stdenv.cc}/bin/${pkgs.pkgsStatic.stdenv.cc.targetPrefix}cc";
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=-static";
          
          # Use system SQLite
          SQLITE3_LIB_DIR = "${pkgs.pkgsStatic.sqlite.out}/lib";
          SQLITE3_INCLUDE_DIR = "${pkgs.pkgsStatic.sqlite.dev}/include";
          SQLITE3_STATIC = "1";
          
          # Override cargo target dir to use musl subdirectory
          preBuild = ''
            export CARGO_TARGET_DIR="target"
          '';
          
          doCheck = false; # Tests don't work well with static linking
          
          # Verify the binary is statically linked
          postInstall = ''
            echo "Checking if binary is statically linked..."
            file $out/bin/replicante
            # Strip the binary to reduce size
            ${pkgs.binutils}/bin/strip $out/bin/replicante
          '';
          
          meta = with pkgs.lib; {
            description = "Autonomous AI Agent (static musl build)";
            license = licenses.mit;
            platforms = [ "x86_64-linux" ];
          };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            bashInteractive
            rustToolchain
            pkg-config
            sqlite
            openssl
            
            # Development tools
            cargo-watch
            cargo-expand
            rust-analyzer
          ];
          
          # Set environment variables for SQLite linking
          SQLITE3_LIB_DIR = "${pkgs.pkgsStatic.sqlite.out}/lib";
          SQLITE3_INCLUDE_DIR = "${pkgs.pkgsStatic.sqlite.dev}/include";
          SQLITE3_STATIC = "1";
          PKG_CONFIG_PATH = "${pkgs.pkgsStatic.sqlite.dev}/lib/pkgconfig";
          
          shellHook = ''
            echo "Replicante development environment"
            echo ""
            echo "Build commands:"
            echo "  cargo build                    - Build development version"
            echo "  cargo build --release          - Build release version"
            echo "  nix build .#replicante-static  - Build static musl binary"
            echo ""
            echo "Run commands:"
            echo "  cargo run                      - Run agent"
            echo "  cargo test                     - Run tests"
            echo ""
            echo "Set environment variables:"
            echo "  export ANTHROPIC_API_KEY=sk-..."
            echo "  export OPENAI_API_KEY=sk-..."
          '';
        };
      }
    );
}