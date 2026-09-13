#!/usr/bin/env node
// The `klyro` command. Finds the binary npm installed for this machine
// and hands the process over to it.
//
// The binaries live
// in the per-platform packages listed under optionalDependencies, one
// of which npm will have installed - that is the whole mechanism.

"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");

// Kept in step with the table in npm/build.mjs. `process.platform` and
// `process.arch` are the same strings npm matched against `os` and
// `cpu` when it decided which package to install, so the name built
// here is the name of the package that is actually on disk.
const SUPPORTED = new Set(["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"]);

const platform = `${process.platform}-${process.arch}`;
const pkg = `klyro-db-${platform}`;

function fail(message) {
    console.error(`klyro: ${message}`);
    process.exit(1);
}

if (!SUPPORTED.has(platform)) {
    // Klyro polls with poll(2) and shuts down out of a POSIX signal
    // handler, so Windows is not a build that exists rather than one
    // that is missing.
    fail(
        `there is no Klyro build for ${platform}.\n` +
            "       macOS and Linux on x64 or arm64 are the supported platforms.\n" +
            "       Elsewhere, run the Docker image or build from source:\n" +
            "         docker run -d -p 7171:7171 ghcr.io/hitesh-s0lanki/klyro\n" +
            "         cargo install klyro",
    );
}

let binary;
try {
    // Resolving package.json and walking up beats resolving the binary
    // path directly: it needs no file extension and no `exports` entry,
    // which is what makes it work on every Node that can run this file.
    binary = path.join(path.dirname(require.resolve(`${pkg}/package.json`)), "bin", "klyro");
} catch {
    fail(
        `the ${pkg} package is missing.\n` +
            "       It is an optional dependency, so an install run with --omit=optional\n" +
            "       or --no-optional will not have it. Reinstall with optional\n" +
            "       dependencies enabled:\n" +
            "         npm install klyro-db",
    );
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

// Klyro writes its dump on SIGTERM and SIGINT, so a signal that stops
// this wrapper has to reach the server or a shutdown loses the
// keyspace. Ctrl-C already reaches both through the terminal; this is
// for the `docker stop` and `kill` cases, where only the parent is
// signalled.
for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) {
    process.on(signal, () => {
        if (child.exitCode === null && child.signalCode === null) {
            child.kill(signal);
        }
    });
}

child.on("error", (error) => fail(`could not run ${binary}: ${error.message}`));

// Reporting a signal death as 128 + signal is what a shell would have
// done, and it keeps `npx klyro && ...` behaving.
child.on("exit", (code, signal) => {
    process.exit(signal ? 128 + (require("node:os").constants.signals[signal] ?? 0) : (code ?? 1));
});
