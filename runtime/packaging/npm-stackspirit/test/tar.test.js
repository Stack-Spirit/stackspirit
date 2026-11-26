"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");

const { extractFile } = require("../lib/tar");
const { makeTarGz } = require("./helpers/tar");

test("extractFile returns the requested regular file", () => {
  const archive = makeTarGz([
    { name: "stackspirit-0.4.0-x86_64-unknown-linux-gnu/README.md", data: "read me" },
    { name: "stackspirit-0.4.0-x86_64-unknown-linux-gnu/stackspirit", data: "binary", mode: 0o755 },
  ]);
  const file = extractFile(
    archive,
    "stackspirit-0.4.0-x86_64-unknown-linux-gnu/stackspirit",
  );
  assert.equal(file.data.toString(), "binary");
  assert.equal(file.mode, 0o755);
});

test("extractFile rejects an absolute path", () => {
  const archive = makeTarGz([{ name: "/etc/passwd", data: "no" }]);
  assert.throws(() => extractFile(archive, "etc/passwd"), /absolute path/);
});

test("extractFile rejects a path with ..", () => {
  const archive = makeTarGz([{ name: "../escape", data: "no" }]);
  assert.throws(() => extractFile(archive, "escape"), /unsafe path/);
});

test("extractFile rejects a symlink at the requested path", () => {
  const archive = makeTarGz([
    { name: "stackspirit-0.4.0-x86_64-unknown-linux-gnu/stackspirit", type: "2" },
  ]);
  assert.throws(
    () => extractFile(archive, "stackspirit-0.4.0-x86_64-unknown-linux-gnu/stackspirit"),
    /not a regular file/,
  );
});

test("extractFile rejects an archive without the requested file", () => {
  const archive = makeTarGz([{ name: "other/stackspirit", data: "binary" }]);
  assert.throws(
    () => extractFile(archive, "stackspirit-0.4.0-x86_64-unknown-linux-gnu/stackspirit"),
    /does not contain/,
  );
});
