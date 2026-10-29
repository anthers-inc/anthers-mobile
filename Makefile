# ─── Anthers Mobile ───
#
# The mobile shell's front door. The commands are the desktop's shape with the Android
# project's requirements folded in: the web build resolves before anything compiles, the
# SDK and NDK have to be present, and `gen/android` has to exist before the first build.
#
# POSIX shells only. Android builds are proven on Linux and iOS needs macOS, so a
# PowerShell branch would be dead weight here.

.PHONY: help setup web init dev emulator emulator-stop emulator-list build build-debug apk check lint format test clean version

# ─── Paths ───

SRC := src-tauri
ANDROID := $(SRC)/gen/android
TAURI := bunx tauri
BUN := bun

# 🚨 **These default rather than trust the environment, and make exports them.**
# They live in `~/.zshenv`, which a shell opened before that file changed has never read —
# so `$(ANDROID_HOME)/platform-tools/adb` used to expand to `/platform-tools/adb` and every
# target failed with a message about the wrong thing. `?=` keeps a value the environment
# does supply, and the fallbacks are the conventional locations this machine installed to.
ANDROID_HOME ?= $(HOME)/Android/Sdk
JAVA_HOME ?= $(HOME)/.local/jdk-21
NDK_HOME ?= $(ANDROID_HOME)/ndk/27.0.12077973
export ANDROID_HOME JAVA_HOME NDK_HOME

# Derived from the three above, so nothing below can be built out of an empty variable.
EMULATOR := $(ANDROID_HOME)/emulator/emulator
ADB := $(ANDROID_HOME)/platform-tools/adb
SDKMANAGER := $(ANDROID_HOME)/cmdline-tools/latest/bin/sdkmanager
AVDMANAGER := $(ANDROID_HOME)/cmdline-tools/latest/bin/avdmanager

# The AVD `make dev` runs against. Override on the command line for a second device.
AVD ?= anthers-pixel
# 🚨 **`swiftshader_indirect`, not `auto`, and this is a fix rather than a preference.**
# `auto` picked host rendering, loaded the machine's GL and Vulkan stack, and crashed the
# emulator seconds after boot: a core dump with `libvulkan_lvp` and `libshadertranslator`
# loaded, and a TLS/resolver recursion inside the crash handler itself. The headless path
# had booted fine, which is why this looked solved — `-no-window` never exercises host
# rendering, so only a windowed boot meets it. Software rendering costs frames, not
# correctness, and a dev emulator that stays up beats one that is fast until it segfaults.
# `GPU=host make emulator` tries hardware again; `GPU=auto` is the emulator's own choice.
GPU ?= swiftshader_indirect

# The emulator's own stderr. It used to go to /dev/null, which is how a crash arrived as
# "it freezes and then crashes" with nothing to read; a crash now leaves a log beside the
# checkout.
EMULATOR_LOG ?= .emulator.log

# Where `android build` leaves its output. The universal directory is what `--apk`
# produces; a per-ABI build lands beside it under the ABI's own name.
APK_DIR := $(ANDROID)/app/build/outputs/apk/universal

# `bunx tauri` resolves the CLI out of node_modules, so every target that runs it needs the
# install to have happened. A file target is what makes that automatic and once-only — the
# alternative was a `make setup` ritual that a fresh clone silently skipped.
node_modules: package.json bun.lock
	@$(BUN) install

# One guard, so a missing SDK is one clear sentence instead of a path that resolves to `/`.
require-sdk:
	@test -d "$(ANDROID_HOME)" || (echo "No Android SDK at $(ANDROID_HOME) — set ANDROID_HOME. See the README's toolchain section."; exit 1)
	@test -x "$(ADB)" || (echo "No adb at $(ADB) — ANDROID_HOME does not look like an SDK."; exit 1)

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
	@if [ -x "$(EMULATOR)" ]; then echo "  emulator   $(EMULATOR)"; else echo "  emulator   missing — sdkmanager --install emulator (only needed to run the app)"; fi
	@if [ -x "$(EMULATOR)" ] && $(AVDMANAGER) list avd 2>/dev/null | grep -q "Name: $(AVD)"; then \
		echo "  avd        $(AVD)"; \
	else \
		echo "  avd        $(AVD) not created — see the README's emulator section"; \
	fi
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

dev: node_modules require-sdk web ## Run the app on a device or emulator
	@test -d "$(ANDROID)" || (echo "src-tauri/gen/android is missing — run 'make init'"; exit 1)
	@$(ADB) devices | grep -q "device$$" || echo "  ! Nothing is attached — run 'make emulator' first (or plug in a phone)."
	@echo "Pointing the app at http://10.0.2.2:8000 — the emulator's alias for this machine."
	@echo "A physical device needs ANTHERS_API_BASE set to this machine's LAN address."
	$(TAURI) android dev

# ─── Emulator ───

# A windowed boot is the default because a phone-sized surface is the thing being built.
# `HEADLESS=1` drops the window, which is what a run with no display — an agent session,
# CI — needs; the app still installs, runs and screenshots.
emulator: require-sdk ## Boot the AVD `make dev` runs against (HEADLESS=1 for no window)
	@test -x "$(EMULATOR)" || (echo "No emulator at $(EMULATOR) — $(SDKMANAGER) --install emulator"; exit 1)
	@$(AVDMANAGER) list avd 2>/dev/null | grep -q "Name: $(AVD)" || (\
		echo "The AVD $(AVD) does not exist. Create it once:"; \
		echo "  $(AVDMANAGER) create avd -n $(AVD) -k \"system-images;android-35;google_apis;x86_64\" --device pixel_8"; \
		exit 1)
	@if [ -n "$$($(ADB) devices | awk '/^emulator-/{print $$1; exit}')" ]; then \
		echo "An emulator is already running."; exit 0; \
	fi
	@$(ADB) start-server >/dev/null 2>&1 || true
	@echo "Booting $(AVD)… (stderr → $(EMULATOR_LOG))"
	@$(EMULATOR) -avd $(AVD) -gpu $(GPU) -no-audio -no-boot-anim -no-snapshot \
		$(if $(HEADLESS),-no-window) >$(EMULATOR_LOG) 2>&1 &
	@$(ADB) wait-for-device
	@echo "Waiting for Android to finish booting…"
	@i=0; while [ "$$($(ADB) shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" != "1" ]; do \
		i=$$((i+1)); \
		if [ $$i -gt 150 ]; then echo "Timed out after five minutes — see $(EMULATOR_LOG) and try 'GPU=host make emulator' or 'make emulator-stop && make emulator'."; exit 1; fi; \
		sleep 2; \
	done; echo "Booted."

emulator-stop: ## Stop the emulator
	@for d in $$($(ADB) devices | awk '/^emulator-/{print $$1}'); do \
		echo "Stopping $$d"; $(ADB) -s $$d emu kill >/dev/null 2>&1 || true; \
	done; true

emulator-list: ## List the AVDs on this machine
	@$(AVDMANAGER) list avd 2>/dev/null | grep -E "^ +Name:" || echo "No AVDs — see the README for the avdmanager command."

# ─── Build ───

build: node_modules require-sdk web ## Build a release APK for every ABI
	@test -d "$(ANDROID)" || (echo "src-tauri/gen/android is missing — run 'make init'"; exit 1)
	$(TAURI) android build --apk
	@$(MAKE) --no-print-directory apk

build-debug: node_modules require-sdk web ## Build a debug APK for aarch64 only (the fast loop)
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
