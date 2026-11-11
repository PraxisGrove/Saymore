export const agent = {
  id: "saymore",
  name: "Saymore",
  bin: "saymore-desktop",
  config: "$LOCALAPPDATA/Saymore/config.json",
  format: "json",
  model: "integrations.magpie.model",
  ua: ["saymore"],
  notice: "Connect or change the model while Saymore is closed, then reopen it and test the Magpie provider in Models → Text polishing.",
}

export function connect({ gateway }) {
  return {
    "integrations.magpie.base_url": gateway.v1,
    "integrations.magpie.api_key": gateway.key,
    "integrations.magpie.profile": "portable",
  }
}
