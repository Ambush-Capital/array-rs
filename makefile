# Makefile for ARRAY-RS Project
# This Makefile manages the build, development, and database operations for the ARRAY-RS project,
# which consists of three main components: blockchain, worker, and API services.

# Configuration Variables
# ---------------------
# Name of the SQLite database file used for storing lending market data
DB_FILE = solana_lending_markets.db

# Project component directories
BLOCKCHAIN_DIR = blockchain
WORKER_DIR = worker
API_DIR = api

# Load environment variables from .env file (if present)
# Required variables:
# - RPC_URL: The Solana RPC endpoint (e.g., https://mainnet.helius-rpc.com/?api-key=your_api_key)
# - KEYPAIR_PATH: Path to your Solana keypair file
-include .env

# Declare all phony targets (targets that don't represent files)
.PHONY: help create-db delete-db run-chain-api run-worker dev-reset dev dev-build install-deps

# Default target when running just 'make'
.DEFAULT_GOAL := help

##@ Development Environment

help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

install-deps: ## Install all required dependencies (rustup, sqlite3, tmux)
	@echo "Installing and verifying project dependencies..."
	@echo "Step 1/3: Checking for rustup and Rust toolchain..."
	@if ! command -v rustup > /dev/null; then \
		echo "rustup not found. Installing rustup and Rust toolchain..."; \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
		source $$HOME/.cargo/env; \
	else \
		echo "✓ rustup is already installed."; \
	fi
	@echo "Step 2/3: Checking for sqlite3..."
	@if ! command -v sqlite3 > /dev/null; then \
		echo "Installing sqlite3..."; \
		brew install sqlite3; \
	else \
		echo "✓ sqlite3 is already installed."; \
	fi
	@echo "Step 3/3: Checking for tmux..."
	@if ! command -v tmux > /dev/null; then \
		echo "Installing tmux..."; \
		brew install tmux; \
	else \
		echo "✓ tmux is already installed."; \
	fi
	@echo "✓ All dependencies are installed and ready!"

##@ Database Management

create-db: ## Create a new SQLite database if it doesn't exist
	@echo "Initializing database..."
	@if [ ! -f $(DB_FILE) ]; then \
		echo "Creating new SQLite database: $(DB_FILE) in $$(pwd)"; \
		echo "CREATE TABLE IF NOT EXISTS tmp_table (id INTEGER);" | sqlite3 $(DB_FILE); \
		echo "✓ Database created successfully!"; \
	else \
		echo "! Database $(DB_FILE) already exists in $$(pwd)"; \
	fi

delete-db: ## Delete the existing SQLite database
	@echo "Database cleanup..."
	@if [ -f $(DB_FILE) ]; then \
		echo "Removing existing database: $(DB_FILE)"; \
		rm $(DB_FILE); \
		echo "✓ Database deleted successfully!"; \
	else \
		echo "! Database $(DB_FILE) does not exist."; \
	fi

##@ Services

run-chain-api: ## Start the blockchain API service (requires RPC_URL in .env)
	@if [ -z "$(RPC_URL)" ]; then \
		echo "❌ Error: RPC_URL is not set. Please add it to your .env file."; \
		exit 1; \
	fi
	@if [ -z "$(KEYPAIR_PATH)" ]; then \
		echo "❌ Error: KEYPAIR_PATH is not set. Please add it to your .env file."; \
		exit 1; \
	fi
	@echo "Starting blockchain API service..."
	cd $(BLOCKCHAIN_DIR) && RPC_URL=$(RPC_URL) KEYPAIR_PATH=$(KEYPAIR_PATH) cargo run -p chain-api

run-worker: ## Start the worker service that processes blockchain data
	@echo "Starting worker service..."
	cd $(WORKER_DIR) && DB_FILE=$(DB_FILE) cargo run

run-api: ## Start the API service that serves processed data
	@echo "Starting API service..."
	cd $(API_DIR) && DB_FILE=$(DB_FILE) cargo run

##@ Development

dev: create-db ## Launch all services (chain-api, worker, and API) in tmux sessions
	@echo "Launching development environment in tmux..."
	@echo "Starting new tmux session 'array-rs' with all services..."
	tmux new-session -d -s array-rs "make run-chain-api"
	tmux split-window -h -t array-rs "make run-worker"
	tmux split-window -v -t array-rs "make run-api"
	tmux select-layout -t array-rs tiled
	@echo "✓ All services started in tmux. Attaching to session..."
	tmux attach-session -t array-rs

dev-build: ## Build all project components without running them
	@echo "Building all project components..."
	@echo "Step 1/3: Building blockchain component..."
	cd $(BLOCKCHAIN_DIR) && cargo build
	@echo "Step 2/3: Building worker component..."
	cd $(WORKER_DIR) && cargo build
	@echo "Step 3/3: Building API component..."
	cd $(API_DIR) && cargo build
	@echo "✓ All components built successfully!"

dev-reset: delete-db create-db ## Reset database to a clean state
	@echo "✓ Database has been reset to initial state"

dev-kill: ## Kill all running project processes
	@echo "Killing all project-related processes..."
	@pids=$$(pgrep -f "chain-api\|worker\|api"); \
	if [ -n "$$pids" ]; then \
		echo "Found processes: $$pids"; \
		echo $$pids | xargs kill -9; \
		echo "✓ All processes terminated"; \
	else \
		echo "No running processes found"; \
	fi
