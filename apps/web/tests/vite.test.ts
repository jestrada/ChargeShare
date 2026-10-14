import { test } from "node:test"
import assert from "node:assert/strict"
import config from "../vite.config.ts"

function serverConfig(mode: string, port: string | undefined) {
  const previous = process.env.CHARGESHARE_API_PORT
  if (port === undefined) delete process.env.CHARGESHARE_API_PORT
  else process.env.CHARGESHARE_API_PORT = port
  try {
    return config({ command: "serve", mode }).server
  } finally {
    if (previous === undefined) delete process.env.CHARGESHARE_API_PORT
    else process.env.CHARGESHARE_API_PORT = previous
  }
}

test("persisted proxy defaults to the loopback API and accepts valid port overrides", () => {
  for (const [port, expected] of [[undefined, 8788], ["1", 1], ["8789", 8789], ["0008789", 8789], ["65535", 65535]] as const) {
    const server = serverConfig("persisted", port)
    assert.equal(server.host, "127.0.0.1")
    assert.equal(server.port, 5174)
    assert.equal(server.strictPort, true)
    assert.deepEqual(server.proxy, { "/api/poc": `http://127.0.0.1:${expected}` })
  }
})

test("persisted proxy refuses non-digit zero and out-of-range API ports", () => {
  for (const port of ["", "0", "0000", "-1", "+8788", "65536", "8788.5", "8e3", " 8788", "8788 ", "8788\n", "８７８８", "8788/path", "9999999999999999999999999999999999"]) {
    assert.throws(() => serverConfig("persisted", port), /CHARGESHARE_API_PORT/)
  }
})

test("fixture preview keeps its own loopback proxy regardless of the persisted API port", () => {
  const server = serverConfig("development", "invalid")
  assert.equal(server.host, "127.0.0.1")
  assert.equal(server.port, 5173)
  assert.deepEqual(server.proxy, { "/api": "http://127.0.0.1:8787" })
})
