# ─── Anthers Mobile ───
#
# The mobile shell's front door. The commands are the desktop's shape with the Android
# project's requirements folded in: the web build resolves before anything compiles, the
# SDK and NDK have to be present, and `gen/android` has to exist before the first build.
#
# POSIX shells only. Android builds are proven on Linux and iOS needs macOS, so a
# PowerShell branch would be dead weight here.

.PHONY: help setup web init dev build build-debug apk check lint format test clean version

# ─── Paths ───

SRC := src-tauri
ANDROID := $(SRC)/gen/android
TAURI := bunx tauri
BUN := bun

# Where `android build` leaves its output. The universal directory is what `--apk`
# produces; a per-ABI build lands beside it under the ABI's own name.
APK_DIR := $(ANDROID)/app/build/outputs/apk/universal

.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort -u | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-16s\033[0m %s\n", $$1, $$2}'

# ─── Setup ───

# Probes rather than installs. The Android SDK and NDK are several gigabytes and belong to
# the machine, not to a make target that someone runs in the middle of something else; the
# README carries the one-time install.
setup: ## Check Rust, Bun, the JDK, the SDK/NDK and the Rust targets, then bun install
	@echo "Anthers Mobile — checking the toolchain"
	@command -v rustc >/dev/null 2>&1 && echo "  rustc      $$(rustc --version)" || (echo "  rustc      MISSING — https://rustup.rs"; exit 1)
	@command -v $(BUN) >/dev/null 2>&1 && echo "  bun        $$($(BUN) --version)" || (echo "  bun        MISSING — https://bun.sh"; exit 1)
	@test -n "$$JAVA_HOME" && test -x "$$JAVA_HOME/bin/javac" && echo "  jdk        $$("$$JAVA_HOME/bin/javac" -version 2>&1)" || (echo "  jdk        MISSING — a JDK (not a JRE) with JAVA_HOME set"; exit 1)
	@test -n "$$ANDROID_HOME" && test -d "$$ANDROID_HOME" && echo "  sdk        $$ANDROID_HOME" || (echo "  sdk        MISSING — ANDROID_HOME"; exit 1)
	@test -n "$$NDK_HOME" && test -d "$$NDK_HOME" && echo "  ndk        $$NDK_HOME" || (echo "  ndk        MISSING — NDK_HOME"; exit 1)
	@missing=""; for t in aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android; do \
		rustup target list --installed | grep -qx "$$t" || missing="$$missing $$t"; \
	done; \
	if [ -n "$$missing" ]; then echo "  targets    MISSING — rustup target add$$missing"; exit 1; else echo "  targets    all four Android targets installed"; fi
	@echo ""
	@$(BUN) install

web: ## Resolve the pinned web build into ./web-dist
	@$(BUN) run scripts/resolve-web.ts

# Only needed once per checkout — `gen/android` is committed, so a fresh clone has it. This
# exists for the case where somebody deleted it, and refuses to overwrite a live one.
init: ## Generate src-tauri/gen/android (only if it is missing)
	@if [ -d "$(ANDROID)" ]; then \
		echo "$(ANDROID) already exists — nothing to do."; \
	else \
		$(TAURI) android init; \
	fi

# ─── Run ───

dev: web ## Run the app on a device or emulator
	@test -d "$(ANDROID)" || (echo "src-tauri/gen/android is missing — run 'make init'"; exit 1)
	@echo "Pointing the app at http://10.0.2.2:8000 — the emulator's alias for this machine."
	@echo "A physical device needs ANTHERS_API_BASE set to this machine's LAN address."
	$(TAURI) android dev

# ─── Build ───

build: web ## Build a release APK for every ABI
	@test -d "$(ANDROID)" || (echo "src-tauri/gen/android is missing — run 'make init'"; exit 1)
	$(TAURI) android build --apk
	@$(MAKE) --no-print-directory apk

build-debug: web ## Build a debug APK for aarch64 only (the fast loop)
	@test -d "$(ANDROID)" || (echo "src-tauri/gen/android is missing — run 'make init'"; exit 1)
	$(TAURI) android build --apk --debug --target aarch64
	@$(MAKE) --no-print-directory apk

apk: ## Print the path of the last built APK
	@ls -1 $(APK_DIR)/debug/*.apk $(APK_DIR)/release/*.apk 2>/dev/null | tail -1 || \
		(echo "No APK built yet — run 'make build-debug' or 'make build'."; exit 1)

# ─── Quality ───

check: ## Run Rust compiler checks for the host target, without building
	@cd $(SRC) && cargo check

lint: ## Run clippy over the shell
	@cd $(SRC) && cargo clippy -- -D warnings

format: ## Format the shell with rustfmt
	@cd $(SRC) && cargo fmt

test: ## Run the shell's Rust tests
	@cd $(SRC) && cargo test

# ─── Versioning ───

# The version lives in three places and they are moved together or not at all. The desktop
# learnt this the hard way and grew a script; here the check is enough until the mobile
# shell earns its own release line, because a disagreement has no other symptom — every
# tool reads its own copy and the build succeeds against whichever one it found.
version: ## Print the three version fields and fail when they disagree
	@P=$$(grep -m1 '"version":' package.json | sed 's/.*"version": "\(.*\)".*/\1/'); \
	C=$$(grep -m1 '^version = ' $(SRC)/Cargo.toml | sed 's/version = "\(.*\)"/\1/'); \
	T=$$(grep -m1 '"version":' $(SRC)/tauri.conf.json | sed 's/.*"version": "\(.*\)".*/\1/'); \
	echo "  package.json     $$P"; \
	echo "  Cargo.toml       $$C"; \
	echo "  tauri.conf.json  $$T"; \
	if [ "$$P" != "$$C" ] || [ "$$P" != "$$T" ]; then \
		echo ""; echo "They disagree — move all three together."; exit 1; \
	fi; \
	echo ""; echo "Agreed: $$P"

# ─── Maintenance ───

# Everything here is generated. `gen/android` itself is committed and is deliberately not
# removed — it is the project Tauri builds, not a build artifact.
clean: ## Remove the resolved web build, Rust output and the Android project's build output
	@echo "Removing build output…"
	@rm -rf web-dist .web-dist.tar.gz
	@rm -rf $(SRC)/target
	@rm -rf $(ANDROID)/app/build $(ANDROID)/build $(ANDROID)/.gradle
	@echo "Clean. gen/android is untouched — it is committed, not generated output."
