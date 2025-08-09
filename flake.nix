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
        
        # Static musl build for deployment
        packages.replicante-static = let
          rustPlatformMusl = pkgs.pkgsMusl.makeRustPlatform {
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
            pkgsMusl.stdenv.cc
          ];
          
          # No external buildInputs needed - everything is bundled
          buildInputs = [];
          
          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsMusl.stdenv.cc}/bin/${pkgs.pkgsMusl.stdenv.cc.targetPrefix}cc";
          
          # Ensure fully static linking
          RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=-static -C link-arg=-static-pie";
          
          # Use bundled SQLite from rusqlite crate
          RUSQLITE_BUNDLED = "1";
          
          # Disable OpenSSL, use rustls instead
          OPENSSL_NO_VENDOR = "0";
          
          doCheck = false; # Tests don't work well with static linking
          
          # Verify the binary is actually static
          postInstall = ''
            echo "Verifying static binary..."
            if ldd $out/bin/replicante 2>&1 | grep -q "not a dynamic executable"; then
              echo "✓ Binary is statically linked"
            else
              echo "⚠ Warning: Binary may have dynamic dependencies:"
              ldd $out/bin/replicante || true
            fi
            
            echo "Binary size: $(du -h $out/bin/replicante | cut -f1)"
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
            
            # For testing MCP servers
            nodejs_20
            python3
          ];
          
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