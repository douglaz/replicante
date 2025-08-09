#!/bin/bash

# Replicante Run Script
# This script helps run Replicante with different configurations

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check for required environment variables
check_env() {
    if [ -z "$ANTHROPIC_API_KEY" ] && [ -z "$OPENAI_API_KEY" ] && [ -z "$OLLAMA_HOST" ]; then
        echo -e "${RED}Error: No LLM API key found${NC}"
        echo "Please set one of:"
        echo "  export ANTHROPIC_API_KEY=sk-..."
        echo "  export OPENAI_API_KEY=sk-..."
        echo "  export OLLAMA_HOST=http://localhost:11434"
        exit 1
    fi
}

# Display current configuration
show_config() {
    echo -e "${GREEN}Replicante Configuration:${NC}"
    
    if [ -n "$ANTHROPIC_API_KEY" ]; then
        echo "  LLM Provider: Anthropic Claude"
    elif [ -n "$OPENAI_API_KEY" ]; then
        echo "  LLM Provider: OpenAI"
    elif [ -n "$OLLAMA_HOST" ]; then
        echo "  LLM Provider: Ollama (local)"
    fi
    
    echo "  Database: ${DATABASE_PATH:-replicante.db}"
    echo "  Log Level: ${RUST_LOG:-info}"
    echo ""
}

# Main execution
case "${1:-run}" in
    build)
        echo -e "${YELLOW}Building Replicante...${NC}"
        cargo build --release
        echo -e "${GREEN}Build complete!${NC}"
        ;;
    
    test)
        echo -e "${YELLOW}Running tests...${NC}"
        cargo test
        echo -e "${GREEN}All tests passed!${NC}"
        ;;
    
    clean)
        echo -e "${YELLOW}Cleaning database and logs...${NC}"
        rm -f replicante.db replicante.db-*
        echo -e "${GREEN}Cleaned!${NC}"
        ;;
    
    run)
        check_env
        show_config
        echo -e "${YELLOW}Starting Replicante...${NC}"
        echo ""
        cargo run --release
        ;;
    
    dev)
        check_env
        show_config
        echo -e "${YELLOW}Starting Replicante in development mode...${NC}"
        RUST_LOG=debug cargo run
        ;;
    
    *)
        echo "Usage: $0 {run|dev|build|test|clean}"
        echo ""
        echo "Commands:"
        echo "  run   - Run Replicante in release mode"
        echo "  dev   - Run Replicante in development mode with debug logging"
        echo "  build - Build release binary"
        echo "  test  - Run tests"
        echo "  clean - Remove database and temporary files"
        exit 1
        ;;
esac