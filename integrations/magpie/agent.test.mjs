import assert from "node:assert/strict"
import test from "node:test"
import { agent, connect } from "./agent.js"

test("exports a reversible connection without touching existing Providers", () => {
  assert.equal(agent.id, "saymore")
  assert.equal(agent.model, "integrations.magpie.model")
  const result = connect({
    gateway: { v1: "http://127.0.0.1:3425/v1", key: "local-attribution" },
    model: { id: "provider/model" },
  })
  assert.equal(result["integrations.magpie.model"], undefined)
  assert.equal(result["integrations.magpie.base_url"], "http://127.0.0.1:3425/v1")
  assert.equal(result["integrations.magpie.api_key"], "local-attribution")
  assert.equal(result["integrations.magpie.profile"], "portable")
  assert.ok(Object.keys(result).every((key) => !key.startsWith("llm_providers") && key !== "active.llm"))
})
