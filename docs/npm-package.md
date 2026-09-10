# The npm packages

Usage lives in the README's "Install from npm" section. This file covers
how the packages are put together, why they are put together that way,
and what it takes to publish a release.

## Why npm at all

Klyro's audience runs Node. The people most likely to want an in-memory
store for an AI agent already have npm in front of them, and asking them
for a Rust toolchain or a Docker daemon to get a dev server up is asking
for more than the task is worth. `npx klyro-db` is the shortest path
from reading about Klyro to talking to it.

## Five packages, one binary each

npm cannot compile a Rust program on install, so a native tool reaches
npm one of two ways.

The first is a `postinstall` script that downloads a binary from GitHub
Releases. It is simple and it breaks in exactly the environments that
matter: CI with a package-manager cache and no network, corporate
proxies, `npm ci --ignore-scripts`, offline installs. The binary is also
outside npm's integrity checking, so the lockfile that pins everything
else pins nothing about the program that actually runs.

The second is what esbuild, swc, and Rollup do, and what Klyro does: one
package per platform, each holding nothing but that platform's binary
and a `package.json` declaring the `os` and `cpu` it is for. A wrapper
package lists all of them under `optionalDependencies`. npm skips an
optional dependency whose `os`/`cpu` don't match the machine, so an
install resolves all five and downloads two - the wrapper and the one
binary. No scripts run, everything is in the lockfile, and an offline
install from cache works like any other dependency.

The published set:

| Package | Contents |
|---|---|
| `klyro-db` | The launcher, ~3 kB |
| `klyro-db-darwin-arm64` | Apple silicon binary |
| `klyro-db-darwin-x64` | Intel Mac binary |
| `klyro-db-linux-arm64` | Linux arm64, glibc |
| `klyro-db-linux-x64` | Linux x64, glibc |

`klyro-db`, not `klyro`: the bare name was taken on npm by an unrelated
project before this one got there. The command it installs is still
`klyro`, and `klyro-db` is a second name for the same launcher so that
`npx klyro-db` needs no guessing about which binary npx should pick.

## What is not published

**Windows.** The event loop is `poll(2)` and shutdown runs out of a
POSIX signal handler, so there is no Windows binary to package. The
launcher says so, and points at Docker and WSL, rather than failing with
a resolution error nobody can act on.

**musl.** A `*-linux-musl` binary is static and would run on glibc and
Alpine alike, which is tempting enough to be worth explaining. It is not
published because musl's allocator is materially slower under the
allocation pattern of a server whose entire job is allocating, and the
`libc` field that would let npm choose between a musl and a glibc
package is not honoured by every installer in use. Alpine users have the
Docker image, which is already musl-static.

## The launcher

`npm/klyro-db/bin/klyro.js` is the only code in the wrapper. It builds
the platform package's name from `process.platform` and `process.arch` -
the same strings npm matched `os` and `cpu` against - resolves that
package's `package.json`, and executes the binary beside it.

It resolves `package.json` and walks up rather than resolving the binary
path directly, because that needs neither a file extension nor an
`exports` entry, and so works on every Node that can load the file.

It forwards `SIGTERM`, `SIGINT`, and `SIGHUP` to the child. Klyro writes
its dump out of those handlers, so a signal that stopped only the
launcher would lose the keyspace. Ctrl-C already reaches both processes
through the terminal; the forwarding is for `kill` and for process
supervisors, which signal the parent alone.

Both failure modes explain themselves: an unsupported platform, and a
platform package missing because the install ran with
`--omit=optional`.

## Building them

`npm/build.mjs` assembles a package into `npm/dist/`, which is
gitignored:

```sh
cargo build --release --target aarch64-apple-darwin
node npm/build.mjs aarch64-apple-darwin   # npm/dist/klyro-db-darwin-arm64
node npm/build.mjs --wrapper              # npm/dist/klyro-db
```

The version always comes from `[package]` in `Cargo.toml`, so the crate,
the Docker tags, and the npm packages cannot drift apart. The wrapper
pins each platform package to that exact version - a launcher paired
with some other release's binary is a bug report nobody can reproduce.

To test a build the way a user gets it, pack first and install the
tarballs. Installing the directories instead makes npm link them, and
Node then resolves the launcher's siblings from the checkout rather than
from the install, which passes for the wrong reason:

```sh
cd $(mktemp -d) && npm init -y
npm pack --pack-destination . ~/klyro/npm/dist/klyro-db-*/ ~/klyro/npm/dist/klyro-db/
npm install ./klyro-db-*.tgz
./node_modules/.bin/klyro 7171
```

## Releasing

`.github/workflows/npm-publish.yml` runs on every merge to `main`. It
reads the version from `Cargo.toml` and asks npm whether it is already
published; if it is, the run stops there. **Bumping `version` in
`Cargo.toml` is what cuts a release** - the same rule the Docker
workflow follows.

Then it builds each target on a runner of that architecture (the one
exception is the Intel Mac build, cross-compiled from an arm runner),
installs the packed tarballs, runs the server through the launcher,
checks `PING`, a `SET`/`GET` round trip, and that `SIGTERM` to the
launcher reaches the server and produces a dump - and only then
publishes. Platform packages go first, because the wrapper depends on
them by exact version and an install landing between the two publishes
would come out with no binary.

CI runs a cut-down version of that same install-and-run test on every
pull request, on Linux x64 only. The launcher resolves a package that
exists only after npm has installed it, so an install is the only test
of it worth having, and finding a broken one at release time is finding
it too late.

### Setup

One repository secret, `NPM_TOKEN`: an npm **automation** token (granular
access tokens work too, scoped to publish `klyro-db` and `klyro-db-*`).
Automation tokens bypass 2FA, which is what a CI publish needs.

The publish job also has `id-token: write`, which lets `npm publish
--provenance` attest that these tarballs were built by this workflow
from this commit. The provenance badge on the npm page comes from that.
