#!/usr/bin/env node
// Assembles the npm packages that carry the Klyro binary.
//
// npm has no way to build a Rust program on install, so Klyro ships the
// way esbuild and swc do: one small package per platform holding
// nothing but the binary, and a wrapper package that lists all of them
// as optional dependencies. npm skips an optional dependency whose `os`
// and `cpu` don't match the machine, so an install pulls exactly one
// binary and no compiler.
//
//   node npm/build.mjs <rust-target>    assemble that platform's package
//   node npm/build.mjs --wrapper        assemble the wrapper package
//
// Both write into npm/dist/<package>/, which is what the release
// workflow publishes. The version always comes from Cargo.toml, so a
// version bump there is the only thing that cuts a release.

import { chmodSync, copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const DIST = join(ROOT, "npm", "dist");

/** The name users install, and the name of the command it puts on PATH. */
const WRAPPER = "klyro-db";
const COMMAND = "klyro";

const REPOSITORY = "https://github.com/Hitesh-s0lanki/klyro";

// Klyro's event loop is poll(2) and its shutdown path is a POSIX signal
// handler, so there is no Windows build to ship. Linux is glibc only:
// a musl binary would run everywhere but pays for it in malloc, and
// this is a server whose whole job is allocation.
const TARGETS = {
    "aarch64-apple-darwin": { os: "darwin", cpu: "arm64" },
    "x86_64-apple-darwin": { os: "darwin", cpu: "x64" },
    "aarch64-unknown-linux-gnu": { os: "linux", cpu: "arm64" },
    "x86_64-unknown-linux-gnu": { os: "linux", cpu: "x64" },
};

/** `klyro-db-linux-x64`, the package that carries one target's binary. */
function packageFor(target) {
    const { os, cpu } = TARGETS[target];
    return `${WRAPPER}-${os}-${cpu}`;
}

/**
 * The version from Cargo.toml's `[package]` section.
 *
 * Read rather than taken as an argument so the npm packages, the Docker
 * tags, and the crate can never disagree about what release this is.
 */
function version() {
    const manifest = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
    const section = manifest.split(/^\[/m).find((part) => part.startsWith("package]"));
    const found = section?.match(/^version\s*=\s*"([^"]+)"/m);

    if (!found) {
        throw new Error("Cargo.toml has no version in [package]");
    }
    return found[1];
}

function write(pkg, files) {
    const dir = join(DIST, pkg);
    rmSync(dir, { recursive: true, force: true });

    for (const [path, contents] of Object.entries(files)) {
        const destination = join(dir, path);
        mkdirSync(dirname(destination), { recursive: true });

        if (typeof contents === "string") {
            writeFileSync(destination, contents);
            continue;
        }

        copyFileSync(contents.copy, destination);
        if (contents.executable) {
            // npm keeps the mode it finds, and losing this bit is how a
            // published package turns into "permission denied".
            chmodSync(destination, 0o755);
        }
    }
    console.log(`npm/dist/${pkg}`);
}

/** The shared half of every package.json we generate. */
function common() {
    return {
        version: version(),
        description: "The high-performance in-memory data server",
        license: "MIT",
        homepage: `${REPOSITORY}#readme`,
        repository: { type: "git", url: `git+${REPOSITORY}.git` },
        bugs: { url: `${REPOSITORY}/issues` },
        engines: { node: ">=18" },
    };
}

function buildPlatform(target) {
    const { os, cpu } = TARGETS[target];
    const pkg = packageFor(target);
    const binary = join(ROOT, "target", target, "release", COMMAND);

    write(pkg, {
        "package.json": `${JSON.stringify(
            {
                name: pkg,
                ...common(),
                description: `The Klyro server binary for ${os} ${cpu}`,
                // What makes npm skip this package on every other
                // machine, and what makes the wrapper's dependency on it
                // safe to declare unconditionally.
                os: [os],
                cpu: [cpu],
                files: ["bin"],
                preferUnplugged: true,
            },
            null,
            2,
        )}\n`,
        [`bin/${COMMAND}`]: { copy: binary, executable: true },
        "LICENSE": { copy: join(ROOT, "LICENSE") },
        "README.md":
            `# ${pkg}\n\n` +
            `The \`${os}-${cpu}\` build of the Klyro server. Installed for you by\n` +
            `[\`${WRAPPER}\`](https://www.npmjs.com/package/${WRAPPER}); there is no reason to\n` +
            `depend on it directly.\n`,
    });
}

function buildWrapper() {
    const optionalDependencies = {};
    for (const target of Object.keys(TARGETS)) {
        // Pinned exactly: a wrapper paired with a different release's
        // binary is a bug report nobody can reproduce.
        optionalDependencies[packageFor(target)] = version();
    }

    write(WRAPPER, {
        "package.json": `${JSON.stringify(
            {
                name: WRAPPER,
                ...common(),
                keywords: ["klyro", "redis", "resp", "database", "in-memory", "cache", "server"],
                // Both names, because `npx klyro-db` should work as
                // readily as the `klyro` command the install leaves
                // behind.
                bin: { [COMMAND]: `bin/${COMMAND}.js`, [WRAPPER]: `bin/${COMMAND}.js` },
                main: "./index.js",
                types: "./index.d.ts",
                files: ["bin", "index.js", "index.d.ts", "memory.js"],
                dependencies: { ioredis: "^5.11.1" },
                optionalDependencies,
            },
            null,
            2,
        )}\n`,
        [`bin/${COMMAND}.js`]: { copy: join(ROOT, "npm", WRAPPER, "bin", `${COMMAND}.js`), executable: true },
        "memory.js": { copy: join(ROOT, "npm", WRAPPER, "memory.js") },
        "index.js": { copy: join(ROOT, "npm", WRAPPER, "index.js") },
        "index.d.ts": { copy: join(ROOT, "npm", WRAPPER, "index.d.ts") },
        "LICENSE": { copy: join(ROOT, "LICENSE") },
        "README.md": { copy: join(ROOT, "npm", WRAPPER, "README.md") },
    });
}

const argument = process.argv[2];

if (argument === "--wrapper") {
    buildWrapper();
} else if (argument in TARGETS) {
    buildPlatform(argument);
} else {
    const known = Object.keys(TARGETS).join("\n  ");
    console.error(`usage: node npm/build.mjs --wrapper | <target>\n\ntargets:\n  ${known}`);
    process.exit(2);
}
