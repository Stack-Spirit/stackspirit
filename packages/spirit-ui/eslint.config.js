import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

const tauriOnly = {
  group: ["@tauri-apps/*"],
  message:
    "@stackspirit/ui is host-agnostic. Implement SpiritBridge in the host app " +
    "instead of importing @tauri-apps/api.",
};

const nodeBuiltins = {
  group: ["node:*", "fs", "path", "child_process", "os"],
  message:
    "@stackspirit/ui browser code must not import Node-only modules. " +
    "Keep host and filesystem work behind SpiritBridge.",
};

export default tseslint.config(
  {
    ignores: ["node_modules", "dist", "coverage"],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/consistent-type-imports": [
        "error",
        { "prefer": "type-imports", "fixStyle": "inline-type-imports" },
      ],
      "no-restricted-imports": ["error", { patterns: [tauriOnly] }],
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/test/**", "**/*.test.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        { patterns: [tauriOnly, nodeBuiltins] },
      ],
    },
  },
  {
    files: ["*.config.{js,ts}", "vitest.config.ts"],
    languageOptions: { globals: { ...globals.node } },
  },
);
