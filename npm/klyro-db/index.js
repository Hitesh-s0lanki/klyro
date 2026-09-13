"use strict";

const { Redis } = require("ioredis");
const { memoryApi } = require("./memory.js");

/** Create a client for a running Klyro server; importing does not start a server. */
exports.createClient = function createClient(options = {}) {
    const client = new Redis({ host: "127.0.0.1", port: 7171, ...options });
    Object.defineProperties(client, {
        memory: { value: memoryApi(client), enumerable: true },
        memoryBuffer: { value: memoryApi(client, true), enumerable: true },
    });
    return client;
};
