"use strict";

const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { targetFor, unsupportedMessage } = require("./targets");

/**
 * Run the vendored binary `name` (stackspirit, vowp, stackspirit-daemon or
 * vowp-daemon) with this process's arguments and stdio, and exit with its code.
 */
function launch(name) {
  const binary = path.join(__dirname, "..", "vendor", name);

  if (!fs.existsSync(binary)) {
    const target = targetFor(process.platform, process.arch);
    let message = `stackspirit: the prebuilt ${name} binary is not installed at ${binary}.\n`;
    if (target) {
      message += `Target: ${process.platform}/${process.arch} (${target}).\n`;
    } else {
      message += `${unsupportedMessage(process.platform, process.arch)}\n`;
    }
    message +=
      "Run `npm rebuild -g stackspirit` after a global install, or reinstall the package,\n" +
      "to download it. If you installed with STACKSPIRIT_SKIP_DOWNLOAD=1, run the\n" +
      "install again without it.\n";
    process.stderr.write(message);
    process.exit(1);
  }

  const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

  child.on("error", (error) => {
    process.stderr.write(`stackspirit: cannot run ${binary}: ${error.message}\n`);
    process.exit(1);
  });

  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, () => {
      try {
        child.kill(signal);
      } catch {
        // The child has already exited; the exit handler sets the final status.
      }
    });
  }

  child.on("exit", (code, signal) => {
    if (signal) {
      const number = os.constants.signals[signal];
      process.exit(number ? 128 + number : 1);
    }
    process.exit(code === null ? 1 : code);
  });
}

module.exports = { launch };
