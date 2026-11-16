#!/usr/bin/env node
// Write dist/latest.json for the in-app updater and place the signed update
// archive next to it with the name the manifest points to.
//
//   node scripts/updater-manifest.mjs --repo OWNER/NAME [--tag vX.Y.Z] [--notes "text"]
//
// Needs a build made with TAURI_SIGNING_PRIVATE_KEY set (it produces
// AGASTHY.app.tar.gz and AGASTHY.app.tar.gz.sig). The installed app checks the
// signature against the public key in tauri.conf.json before installing.

import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
};

const repo = arg("--repo");
if (!repo || !/^[\w.-]+\/[\w.-]+$/.test(repo)) {
  console.error("usage: updater-manifest.mjs --repo OWNER/NAME [--tag vX.Y.Z] [--notes text]");
  process.exit(2);
}
const version = JSON.parse(readFileSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8")).version;
const tag = arg("--tag") ?? `v${version}`;
if (tag !== `v${version}`) {
  console.error(`tag ${tag} does not match the app version ${version} in tauri.conf.json`);
  process.exit(1);
}

const bundle = join(root, "target/release/bundle/macos");
const archive = join(bundle, "AGASTHY.app.tar.gz");
const sig = `${archive}.sig`;
if (!existsSync(archive) || !existsSync(sig)) {
  console.error("no signed update archive: build with TAURI_SIGNING_PRIVATE_KEY set (see docs/releasing.md)");
  process.exit(1);
}

const asset = `AGASTHY_${version}_aarch64.app.tar.gz`;
mkdirSync(join(root, "dist"), { recursive: true });
copyFileSync(archive, join(root, "dist", asset));

const manifest = {
  version,
  notes: arg("--notes") ?? `AGASTHY ${version}. See the release notes on GitHub.`,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": {
      signature: readFileSync(sig, "utf8").trim(),
      url: `https://github.com/${repo}/releases/download/${tag}/${asset}`,
    },
  },
};
writeFileSync(join(root, "dist/latest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`dist/latest.json → ${manifest.platforms["darwin-aarch64"].url}`);
