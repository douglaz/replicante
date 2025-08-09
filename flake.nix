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
        packages.replicante-static = pkgs.pkgsMusl.rustPlatform.buildRustPackage {
          pname = "replicante-static";
          version = "0.1.0";
          src = ./.;
          
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          
          # Use bundled SQLite from rusqlite crate
          RUSQLITE_BUNDLED = "1";
          
          doCheck = false; # Tests don't work well with static linking
          
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