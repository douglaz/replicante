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
        
        # Static musl build - following cyberkrill approach exactly
        packages.replicante-static = let
          rustToolchainMusl = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" ];
            targets = [ "x86_64-unknown-linux-musl" ];
          };
          rustPlatformMusl = pkgs.makeRustPlatform {
            cargo = rustToolchainMusl;
            rustc = rustToolchainMusl;
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
            rustToolchainMusl
            pkgsStatic.stdenv.cc
          ];
          
          buildInputs = with pkgs.pkgsStatic; [
            sqlite
          ];
          
          # Environment variables for static SQLite
          SQLITE3_LIB_DIR = "${pkgs.pkgsStatic.sqlite.out}/lib";
          SQLITE3_INCLUDE_DIR = "${pkgs.pkgsStatic.sqlite.dev}/include";
          SQLITE3_STATIC = "1";
          PKG_CONFIG_PATH = "${pkgs.pkgsStatic.sqlite.dev}/lib/pkgconfig";
          
          # Force cargo to use the musl target from .cargo/config.toml
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsStatic.stdenv.cc}/bin/${pkgs.pkgsStatic.stdenv.cc.targetPrefix}cc";
          CC_x86_64_unknown_linux_musl = "${pkgs.pkgsStatic.stdenv.cc}/bin/${pkgs.pkgsStatic.stdenv.cc.targetPrefix}cc";
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=-static";
          
          # Override buildPhase to use the correct target
          buildPhase = ''
            runHook preBuild
            
            echo "Building with musl target for static linking..."
            cargo build \
              --release \
              --target x86_64-unknown-linux-musl \
              --offline \
              -j $NIX_BUILD_CORES
            
            runHook postBuild
          '';
          
          installPhase = ''
            runHook preInstall
            
            mkdir -p $out/bin
            cp target/x86_64-unknown-linux-musl/release/replicante $out/bin/
            
            runHook postInstall
          '';
          
          # Ensure static linking
          doCheck = false; # Tests don't work well with static linking
          
          # Verify the binary is statically linked
          postInstall = ''
            echo "Checking if binary is statically linked..."
            file $out/bin/replicante
            if ldd $out/bin/replicante 2>&1 | grep -E "(not a dynamic executable|statically linked)"; then
              echo "✅ Binary is statically linked"
            else
              echo "❌ Binary is dynamically linked:"
              ldd $out/bin/replicante || true
              exit 1
            fi
            # Strip the binary to reduce size
            ${pkgs.binutils}/bin/strip $out/bin/replicante
          '';
          
          meta = with pkgs.lib; {
            description = "Autonomous AI Agent (static musl build)";
            license = licenses.mit;
            platforms = [ "x86_64-linux" ];
          };
        };

        # Apps for easy running
        apps = {
          # Default app: run replicante
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/replicante";
          };
          
          # Ollama setup app
          ollama-setup = {
            type = "app";
            program = "${pkgs.writeShellScript "ollama-setup" ''
              set -e
              
              echo "🤖 Replicante Ollama Setup"
              echo "=========================="
              echo ""
              
              # Check if Docker is available
              if ! command -v docker &> /dev/null; then
                echo "❌ Docker not found. Please install Docker first."
                echo "   Ubuntu/Debian: sudo apt install docker.io docker-compose"
                echo "   macOS: brew install docker docker-compose"
                exit 1
              fi
              
              # Check if Docker Compose is available
              if ! command -v docker-compose &> /dev/null; then
                echo "❌ Docker Compose not found. Please install Docker Compose first."
                exit 1
              fi
              
              # Check if we're in the right directory
              if [ ! -f "docker-compose.ollama.yml" ]; then
                echo "❌ docker-compose.ollama.yml not found."
                echo "   Please run this from the replicante repository root."
                exit 1
              fi
              
              echo "✅ Docker and Docker Compose found"
              echo ""
              
              # Check if Ollama is running
              if curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
                echo "✅ Ollama is already running"
              else
                echo "🚀 Starting Ollama with Docker Compose..."
                docker-compose -f docker-compose.ollama.yml up -d ollama
                
                echo "⏳ Waiting for Ollama to start..."
                for i in {1..30}; do
                  if curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
                    echo "✅ Ollama is ready"
                    break
                  fi
                  if [ $i -eq 30 ]; then
                    echo "❌ Ollama failed to start within 30 seconds"
                    exit 1
                  fi
                  sleep 1
                done
              fi
              
              echo ""
              echo "📦 Checking for Llama model..."
              
              # Check if we have a model
              if docker exec replicante-ollama ollama list 2>/dev/null | grep -q "llama3.2:3b"; then
                echo "✅ Llama 3.2 3B model found"
              else
                echo "📥 Downloading Llama 3.2 3B model (this may take a few minutes)..."
                docker exec replicante-ollama ollama pull llama3.2:3b
                echo "✅ Model downloaded"
              fi
              
              echo ""
              echo "🛠️  Starting Replicante assistant..."
              docker-compose -f docker-compose.ollama.yml up -d
              
              echo ""
              echo "🎉 Setup complete! Your AI assistant is running."
              echo ""
              echo "📊 Monitor your assistant:"
              echo "   docker-compose -f docker-compose.ollama.yml logs -f replicante"
              echo ""
              echo "🔍 Check status:"
              echo "   docker-compose -f docker-compose.ollama.yml ps"
              echo ""
              echo "🛑 Stop everything:"
              echo "   docker-compose -f docker-compose.ollama.yml down"
              echo ""
              echo "🗄️  View assistant's thoughts:"
              echo "   sqlite3 replicante-ollama.db \"SELECT * FROM decisions ORDER BY created_at DESC LIMIT 5;\""
            ''}";
          };
          
          # Quick Ollama start with Nix
          ollama-nix = {
            type = "app";
            program = "${pkgs.writeShellScript "ollama-nix" ''
              set -e
              
              echo "🤖 Starting Replicante with Ollama (Nix)"
              echo "======================================="
              echo ""
              
              # Check if Ollama is running
              if ! curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
                echo "❌ Ollama not running. Please start Ollama first:"
                echo "   ollama serve"
                echo ""
                echo "   Then pull a model:"
                echo "   ollama pull llama3.2:3b"
                exit 1
              fi
              
              echo "✅ Ollama is running"
              
              # Check for config
              if [ ! -f "config.toml" ]; then
                echo "📝 Creating config from Ollama example..."
                cp config-ollama-example.toml config.toml
                echo "✅ Config created: config.toml"
              fi
              
              echo ""
              echo "🚀 Starting Replicante assistant..."
              echo "   Press Ctrl+C to stop"
              echo ""
              
              # Run in development mode
              nix develop -c cargo run --release
            ''}";
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
            # Set up Git hooks if not already configured
            if [ -d .git ] && [ -d .githooks ]; then
              current_hooks_path=$(git config core.hooksPath || echo "")
              if [ "$current_hooks_path" != ".githooks" ]; then
                echo "📎 Setting up Git hooks for code quality checks..."
                git config core.hooksPath .githooks
                echo "✅ Git hooks configured automatically!"
                echo "   • pre-commit: Checks code formatting"
                echo "   • pre-push: Runs formatting and clippy checks"
                echo ""
                echo "To disable: git config --unset core.hooksPath"
                echo ""
              fi
            fi
            
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