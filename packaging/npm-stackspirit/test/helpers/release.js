"use strict";

const crypto = require("node:crypto");

const { makeTarGz } = require("./tar");

const ALL_TARGETS = [
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
];
const MEMBERS = ["stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon"];

function binaryFor(name, version) {
  return Buffer.from(`#!/bin/sh\nprintf '${name} ${version}\\n'\n`, "utf8");
}

/** A private archive; `omit` lists members left out of it. */
function archiveFor(version, target, omit = []) {
  const root = `stackspirit-${version}-${target}`;
  const entries = MEMBERS.filter((name) => !omit.includes(name)).map((name) => ({
    name: `${root}/${name}`,
    data: binaryFor(name, version),
    mode: 0o755,
  }));
  entries.push({ name: `${root}/LICENSE`, data: "MIT\n" });
  entries.push({ name: `${root}/README.md`, data: "StackSpirit test release\n" });
  return makeTarGz(entries);
}

function sha256(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

/**
 * Build the URL-path -> bytes map for a fake private release tree.
 * `corruptTarget` changes that archive after its checksum is written;
 * `omitMembers` builds archives that lack the named binaries.
 */
function buildRelease(options) {
  const {
    version,
    targets = ALL_TARGETS,
    corruptTarget = null,
    omitMembers = [],
  } = options;
  const files = new Map();
  const sumLines = [];

  for (const target of targets) {
    const name = `stackspirit-${version}-${target}.tar.gz`;
    const bytes = archiveFor(version, target, omitMembers);
    const hash = sha256(bytes);
    files.set(`/v${version}/${name}`, bytes);
    files.set(
      `/v${version}/${name}.sha256`,
      Buffer.from(`${hash}  ${name}\n`, "utf8"),
    );
    sumLines.push(`${hash}  ${name}`);
  }
  files.set(
    `/v${version}/SHA256SUMS`,
    Buffer.from(`${sumLines.join("\n")}\n`, "utf8"),
  );

  if (corruptTarget !== null) {
    const name = `stackspirit-${version}-${corruptTarget}.tar.gz`;
    const original = files.get(`/v${version}/${name}`);
    files.set(
      `/v${version}/${name}`,
      Buffer.concat([original, Buffer.from("corrupted", "utf8")]),
    );
  }
  return files;
}

module.exports = { ALL_TARGETS, MEMBERS, binaryFor, buildRelease, sha256 };
