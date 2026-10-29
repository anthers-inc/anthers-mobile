# Anthers Mobile

The mobile app for [Anthers](https://anthers.org) — the whole platform on a phone, for
Android and iOS.

**This repository is the shell, and there is no UI in it.** The thing inside the window is the
platform's web app, bundled whole; what the shell adds is what a browser tab cannot do — a real
installed app, a session that survives without cookies, a custom-scheme deep link, and a native
surface beside the web for whatever earns one later. Everything inside it is the web
experience, unchanged: the one-to-one shape is the shell's contract, and
`tauri.conf.json`'s `externalBin` must stay absent.

It is the same construction as [`anthers-desktop`](https://github.com/anthers-inc/anthers-desktop)
and a separate repository for the same reason: the release machinery has nothing in common. The
desktop ships installers to GitHub Releases and updates itself in place; a phone app is installed
from a store or from an `.apk`, and neither can update itself the way the desktop's signed
updater does. What the two do share is the **web build** — the same pinned, signature-verified
platform release — so one platform release reaches every surface at once.

**Android comes first.** It can be distributed outside a store, where an `.apk` on a release
sits beside how the desktop already ships. iOS needs Apple's payment rules answered before it can
exist at all, which is a decision rather than a build step.

## Status: first pass

Built and verified on 2026-10-29: the Android project is generated and committed, the web build
resolves from the pinned release with its signature checked, and
`src-tauri/gen/android/app/build/outputs/apk/universal/debug/` holds a debug APK that embeds the
whole web app.

**What it does not have yet.** No sign-in: the injected runtime returns a null token, so the app
browses what a signed-out reader sees. No deep links, no keystore token, no phone-specific
layout — the bundled web app is the desktop layout at phone width. Those are the milestones on
the Roadmap's Anthers Mobile lane.

## Getting the web app

`./web-dist` is what Tauri bundles, and `scripts/resolve-web.ts` fills it from the first source
that works:

1. **`ANTHERS_WEB=/path/to/dist`** — an explicit, already-built dist. Overrides all else.
2. **A sibling `../anthers` (or `../Anthers`) checkout** — built on demand. The local-dev path.
3. **The pinned release asset** — downloaded from the platform's releases. What CI and a clean
   machine use. The pin is `anthersWeb` in `package.json`, naming a promoted platform release tag
   (`v2026.10.34`).

Every pinned download is verified before it is installed against the Ed25519 key committed at
`scripts/web-dist-signing.pub`, and fails closed. The signature ties a download to a key CI holds,
so a compromised release pipeline cannot hand the shell a tampered bundle. The format's owner is
the platform repository's `scripts/sign-web-dist.ts`.

The sibling lookup accepts both spellings because the desktop's resolver checks only the
capitalized one and this machine's platform checkout is lowercase — so the desktop silently falls
through to the pinned asset instead of building the tree beside it.

## Running it

The toolchain this repository needs, none of which is a Rust or Node concern:

| Piece | Version used | Where |
|---|---|---|
| JDK | 21 (Temurin) | `JAVA_HOME` |
| Android SDK | platform 35, build-tools 35.0.0, platform-tools | `ANDROID_HOME` |
| Android NDK | 27.0.12077973 | `NDK_HOME` |
| Rust targets | `aarch64-linux-android`, `armv7-linux-androideabi`, `i686-linux-android`, `x86_64-linux-android` | `rustup target add …` |

The Makefile defaults those three to the conventional locations above and exports them to every
command it runs, so `make` works in a shell that has never read them — and a value already in
the environment wins over the default. The export below is what those defaults mirror; set it
only when a machine keeps its toolchain somewhere else.

```sh
export JAVA_HOME="$HOME/.local/jdk-21"
export ANDROID_HOME="$HOME/Android/Sdk"
export NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"

make setup          # probe the toolchain, then bun install
make run            # build the bundled app, install it, start it — needs nothing else
make dev            # hot reload against the platform's dev server (see below)
make build-debug    # the APK on its own, for aarch64
make build          # a release APK for every ABI
```

**There are two loops, and they differ in what has to be running.** `make run` bundles the
pinned web build into the APK and installs it, so it works by itself — that is the loop for
checking the shell, the emulator, or anything where the web app is not the thing being changed.
`make dev` points the webview at this machine's `:3000` through the emulator's `10.0.2.2` alias,
so **`make dev` in `~/Anthers/anthers` has to be running first**; the target says so and stops if
nothing answers there. Both boot the emulator themselves, because Tauri's own dev runner would
otherwise start one with the emulator's default GPU settings.

`make help` lists the rest: `check`, `lint`, `format`, `test`, `version`, `clean`. `make
version` prints the three version fields and fails when they disagree, which is the only
symptom a disagreement has — every tool reads its own copy and the build succeeds against
whichever one it found.

### The emulator

`make emulator` boots the AVD named `anthers-pixel` and waits until Android has finished
starting; `make emulator-stop` shuts it down, and `make emulator-list` shows what exists. Create
the AVD once, after the emulator package is installed:

```sh
sdkmanager --install "emulator" "system-images;android-35;google_apis;x86_64"
avdmanager create avd -n anthers-pixel -k "system-images;android-35;google_apis;x86_64" --device pixel_8
```

A window is the default because a phone-shaped surface is the thing being built. `make emulator
HEADLESS=1` boots without one, which is what a session with no display needs — the app still
installs, runs and screenshots through `adb`.

Rendering is **software by default** (`GPU=swiftshader_indirect`). `auto` picked host rendering and
crashed this machine's emulator seconds after a windowed boot, inside its own GL and Vulkan stack —
a core dump with `libvulkan_lvp` and `libshadertranslator` loaded. The headless path had looked fine
because `-no-window` never exercises host rendering. `GPU=host make emulator` tries hardware again.
Every boot is a cold one (`-no-snapshot`), so a crash cannot leave a saved state that freezes the
next start, and the emulator's stderr goes to `.emulator.log` rather than nowhere.

`android dev` and `android build` compile the Rust half for each target, so the first run of
either is slow. `--target aarch64` builds one ABI instead of four and is what the first pass was
verified with; a release builds all of them.

Debug builds talk to `http://10.0.2.2:8000` — the Android emulator's alias for the host machine —
so `make dev` in the platform repository is reachable with no configuration. A physical device
needs the host's LAN address passed as `ANTHERS_API_BASE` at build time. Release builds talk to
`https://anthers.org`.

## How the webview reaches the API

A bundled webview on Android serves from `http://tauri.localhost`, where the API origin cannot be
sniffed from the document's host and the `.anthers.org` session cookie is never sent. The shell
defines `globalThis.__ANTHERS_DESKTOP__` before any app JS — the seam `apiBaseUrl()` in
`@anthers/web-shared` already reads — through the plugin `js_init_script`, which runs on every
platform without depending on when a window exists.

That origin is already admitted by the API's CORS and CSRF allow-lists, because Android produces
the same origin Windows does. No platform change was needed for it.

**The seam's name is debt.** It is named for the desktop and this shell borrows it, so
`isDesktop()` answers true inside the app and any desktop-only branch in the web app runs on a
phone. Renaming it to a platform-tagged runtime (`__ANTHERS_NATIVE__` carrying
`platform: "android" | "ios" | "desktop"`) is a platform-repository change and the first thing the
mobile work needs there.

## Layout

```
Makefile             The front door: setup, web, dev, emulator, build, check, lint, test
src-tauri/
  src/lib.rs         The shell: the injected runtime, and the mobile entry point
  src/main.rs        The desktop binary that calls the same run(), for `cargo check`
  tauri.conf.json    Window, identifier, the web-dist path
  gen/android/       Generated by `tauri android init`, committed — the Gradle project
scripts/             resolve-web.ts and the signature verifier
web-dist/            The web app (gitignored, resolved in — never committed)
```

Two config paths resolve from different directories, and it fails late: `frontendDist` and
`licenseFile` resolve from `src-tauri/`, while `beforeBuildCommand` runs from the app directory.
`cargo check` invokes neither, so a broken path survives it.

## Releasing

```sh
make version                                  # the three fields must agree
git tag v2026.10.0 && git push origin v2026.10.0
```

The `release` workflow builds a signed APK for every ABI and opens a **draft** release named
`Anthers Mobile <version>` with the APK attached. **Publishing the draft is the release** — until
then nothing has been handed out. A `workflow_dispatch` run builds the same APK and attaches it
as a run artifact without touching any release, which is how the pipeline is proven between tags.

The signing key's durable home is Bitwarden Secrets in `Anthers Prod` — `ANTHERS_MOBILE_ANDROID_KEY_BASE64`,
`…_KEY_ALIAS` and `…_KEY_PASSWORD` — with the working copy in this repository's `ANDROID_KEY_*`
secrets for the workflow to read. Its certificate fingerprint is what an app link's
`assetlinks.json` has to name. Losing the key means the installed base cannot be updated over:
rotation is a new application ID or a Play upload key reset.

## Licensing

Anthers is Apache-2.0 and `bundle.licenseFile` points at the repository `LICENSE`.
