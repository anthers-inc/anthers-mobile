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

**What it has now.** Sign-in through the system browser with the token held in the Android
keystore (`2026.11.0`), verified `anthers.org` app links that return the OAuth callback to the
app, and the window-insets bridge that keeps the web chrome clear of the status bar, camera
cutout and gesture bar. **No phone-specific layout yet**: the bundled web app is still the
platform's desktop layout at phone width, which is the web client's work rather than the
shell's.

## Getting the web app

`./web-dist` is what Tauri bundles, and `scripts/resolve-web.ts` fills it from the first source
that works:

1. **`ANTHERS_WEB=/path/to/dist`** — an explicit, already-built dist. Overrides all else.
2. **A sibling `../anthers` (or `../Anthers`) checkout** — built on demand. The local-dev path.
3. **The pinned release asset** — downloaded from the platform's releases. What CI and a clean
   machine use. The pin is `anthersWeb` in `package.json`, naming a promoted platform release tag
   (`v2026.11.0`).

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
defines `globalThis.__ANTHERS_NATIVE__` with `platform: "android"` before any app JS — the
platform-tagged seam `apiBaseUrl()`, `isNative()` and `isDesktop()` read — through the plugin
`js_init_script`, which runs on every platform without depending on when a window exists. The
desktop carries the same shape with `platform: "desktop"`, which is what stops a phone from
inheriting the desktop's home preference or its settings section.

That origin is already admitted by the API's CORS and CSRF allow-lists, because Android produces
the same origin Windows does. No platform change was needed for it.

## The content security policy

`src-tauri/tauri.conf.json` carries the policy a release build runs under, in `csp`, and
the looser one a debug build runs under in `devCsp`. Android is the platform where Tauri
proxies the dev server, so the dev policy is the one a developer actually meets. The
release policy defaults to the bundle's own origin, allows scripts from the bundle plus
Stripe's payment script, images from any HTTPS address because a post may point at one,
media from the API origin and the storage endpoint a delivery redirect lands on, frames
from any HTTPS address because a creator's embedded build is served from one, and allows
no embedded object and no base URI at all. Tauri appends the hashes and nonces its own
assets need at build time, so the policy does not list the app's files by name.

Two of its entries are deployment constants rather than app code: the API origin and the
R2 endpoint that presigned uploads go straight to and that media delivery redirects to.
A build pointed at another origin with `ANTHERS_API_BASE` needs that origin added to
`connect-src`, and moving the storage account is the same kind of change — it has to move
in `media-src` beside `connect-src`, or the bytes are blocked after the API has admitted
the request. Either one shows up as a blocked request in the window rather than a failed
build.

## Safe areas

The app targets SDK 37 and calls `enableEdgeToEdge()`, and Android 15 and later enforce
edge-to-edge anyway, so the WebView spans the whole screen — under the status bar, the
navigation bar and any camera cutout. An Android WebView does not report the system bars through
`env(safe-area-inset-*)`: until WebView 144 the values carry only the display cutout, and the
status and navigation bars are invisible to CSS. `MainActivity` therefore measures
`WindowInsetsCompat.Type.systemBars() or displayCutout()` and publishes the four edges in CSS
pixels:

- `__ANTHERS_INSETS__.get()` is the pull, read once on `DOMContentLoaded` by the injected init
  script. That is the path that makes a cold start correct even when an earlier push landed in
  the blank document and was discarded with it.
- `__ANTHERS_APPLY_INSETS__(payload)` is the push, called when the insets change — rotation,
  a switch between gesture and three-button navigation, and the first pass after layout.

The web app turns those values into `--anthers-inset-top`, `--anthers-inset-bottom`,
`--anthers-inset-left` and `--anthers-inset-right`; the shared safe-area utilities in
`apps/web`'s `globals.css` read them, with `env(safe-area-inset-*)` as the fallback for iOS and
for a WebView new enough to report the system bars itself. The topbar's strip, the side padding
and the bottom padding belong to the web app's shell, not to any page, so every route gets the
same safe frame.

## Signing in

The shell never asks for a password. The sign-in screen asks for a handle exactly as `/login`
does, and `/api/auth/desktop/start` registers a PKCE challenge for this attempt. The system
browser then opens `/desktop/authorize?challenge=…&redirect=mobile&handle=…`, and:

- **A browser with no Anthers session** starts the OAuth at whichever server holds the handle,
  carrying the app's challenge in the OAuth state. The server mints the app's one-time code at
  the callback and sends the browser to `https://anthers.org/auth/callback?code=…`. Nothing
  asks the person to approve the device a second time: the identity server's own consent
  screen is the grant.
- **A browser already signed in** mints the code on the authorize page and goes to the same
  place.

That landing page hands the code to the app — over the verified `https://anthers.org` link,
which the release certificate is registered for, or over `anthers://` from the page itself
when verification does not take it. The shell then redeems the code for a session token and
keeps it in the Android keystore. From there the app is signed in, and stays signed in across
restarts.

Five things about that handoff are load-bearing and easy to undo:

- 🚨 **The browser is chosen with this app excluded.** The shell verifies `anthers.org` app
  links so the callback can return to it, which also makes it a handler for the authorize page
  it is handing over. A plain `ACTION_VIEW` resolves back into this app, whose activity is
  `singleTask`, so the window sits still and the handoff does nothing with no error to show.
  `BrowserPlugin.kt` names a browser by its Custom Tabs service and targets it explicitly,
  which needs no URL matching at all; the manifest's `<queries>` block is what lets Android 11
  and later see browsers. Both survive `tauri android build`, which regenerates only the
  deep-link intent filter.
- 🚨 **The PKCE verifier is on disk, not in memory.** Android reclaims this app while the
  person is in the browser, so a verifier that lived in a `Mutex` was gone when the code came
  back. It is written to the app's private data directory with its challenge when the server
  accepts the start, and consumed on redemption.
- 🚨 **The app asks the server whether it is signed in, rather than waiting for the code to
  arrive over a link.** `await_sign_in` polls `/api/auth/desktop/poll/:challenge`, so a browser
  that never hands off, or a process Android reclaimed, still finishes. A code is single-use
  and the server burns its row on any redemption attempt, so **the poll is the phone's only
  redeemer**; the callback link's one job on Android is to open the app.
- 🚨 **The page pulls its session at boot; the shell must not push it.** The keystore read
  blocks, so the shell answers a `session_token` command when the page asks. The old push ran
  when the read finished and often landed in the blank document the real page then replaced,
  which is how a valid session rendered as a signed-out app.
- 🚨 **After a successful redemption the shell reloads the webview.** The exchange usually
  finishes while the browser still owns the screen, and Android freezes a backgrounded
  WebView — a queued promise or state update there may never run. The reloaded page pulls the
  token on boot, which cannot be mistimed.

## Layout

```
Makefile             The front door: setup, web, dev, emulator, build, check, lint, test
src-tauri/
  src/lib.rs         The shell: the injected runtime, and the mobile entry point
  src/main.rs        The desktop binary that calls the same run(), for `cargo check`
  tauri.conf.json    Window, identifier, the web-dist path
  icons/             Packaging icons, cut by `bunx tauri icon` from icons/source/
  gen/android/       Generated by `tauri android init`, committed — the Gradle project
scripts/             resolve-web.ts and the signature verifier
web-dist/            The web app (gitignored, resolved in — never committed)
```

Two config paths resolve from different directories, and it fails late: `frontendDist` and
`licenseFile` resolve from `src-tauri/`, while `beforeBuildCommand` runs from the app directory.
`cargo check` invokes neither, so a broken path survives it.

## Icons

The app wears the brand's 1:1 icon, the bee on cream the site uses everywhere, and
`src-tauri/icons/source/` holds the copy the cut reads: `anthersicon_1x1_light.png`, taken from the
platform's `packages/brand/logo/preps/`, beside `app-icon.json`, the manifest that names it. Cut
every size from those two:

```sh
bunx tauri icon src-tauri/icons/source/app-icon.json
```

The command writes `src-tauri/icons/` for the desktop and iOS sets and
`src-tauri/gen/android/app/src/main/res/` for the launcher's `mipmap` icons, the adaptive icon's
foreground and its background color. Everything it writes is committed, so a build never needs an
image library or a network call. **Run it after `tauri android init` as well as after a change to the
art**, because that command writes the Android template's own icons, and those are what the app shows
until this runs.

**The adaptive icon's foreground is `app-icon-fg.png`, the same art at 80% of the canvas, and that
scale is the point.** A launcher draws an adaptive icon from a 108dp layer but only shows the middle
72dp of it, so a mark drawn to the layer's edge is a mark the mask cuts: at full size the bee's own
bounds are 67% of the canvas, which is the whole visible width, and its head and tail land on the
mask. At 80% the bee is about 54% of the layer, leaving margin under every mask: the circle, the
squircle, and the launchers that draw icons a little larger than the shape they crop to.

**The manifest's `bg_color` is the icon's own cream, `#F5F3E3`, and not white.** An adaptive icon's
two layers have to fill whatever shape the launcher crops them to, so the artwork's transparent
corners take that color, and white leaves a pale crescent inside the mask. With the cream, the circle
the art is drawn on disappears into its own background and the launcher's mask is the only edge the
icon has.

## Releasing

`/promote-mobile` is the whole flow as one resumable procedure: the state audit, the computed
version bump with the platform web pin, one PR, the tag, the build watch, the draft's verification,
and the publish. It runs from the main checkout on `main`, and only on an explicit decision to ship.
The command file (`~/.config/opencode/commands/promote-mobile.md`) is the procedure's authority; this
section is the shape of it.

**The version is computed, never typed.** `bun run scripts/promote-version.ts` writes `package.json`,
`src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.lock` together, and its
`--check` is what CI runs, so four fields drifting apart fails a pull request rather than a release.
`make version` is the same agreement check for a person reading a tree.

`anthersWeb` in `package.json` names the promoted platform release the shell bundles, and the promote
moves it to the platform's latest: one platform release reaching every surface at once is the point
of the pin, and a release that left it behind would ship an old web app under a new number.

The `release` workflow builds a signed APK for every ABI and opens a **draft** release named
`Anthers Mobile <version>` with `Anthers_<version>.apk` attached, the version read from the tree
rather than from the tag's text. **Publishing the draft is the release**: until then nothing has been
handed out. A `workflow_dispatch` run builds the same APK and attaches it as a run artifact without
touching any release, which is how the pipeline is proven between tags.

The signing key's durable home is Bitwarden Secrets in `Anthers Prod` — `ANTHERS_MOBILE_ANDROID_KEY_BASE64`,
`…_KEY_ALIAS` and `…_KEY_PASSWORD` — with the working copy in this repository's `ANDROID_KEY_*`
secrets for the workflow to read. Its certificate fingerprint is what an app link's
`assetlinks.json` has to name. Losing the key means the installed base cannot be updated over:
rotation is a new application ID or a Play upload key reset.

## Licensing

Anthers is Apache-2.0 and `bundle.licenseFile` points at the repository `LICENSE`.
