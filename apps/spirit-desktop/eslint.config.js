import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

const tauriOnly = {
  group: ["@tauri-apps/*"],
  message:
    "Only src/bridge/TauriBridge.ts may import @tauri-apps/api. " +
    "All other code must go through the SpiritBridge interface.",
};

export default tseslint.config(
  {
    ignores: [
      "dist",
      "node_modules",
      "playwright-report",
      "test-results",
      "coverage",
      "src-tauri/target",
    ],
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
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    rules: { "no-restricted-imports": ["error", { patterns: [tauriOnly] }] },
  },
  {
    files: ["src/bridge/TauriBridge.ts"],
    rules: { "no-restricted-imports": "off" },
  },
  {
    files: ["*.config.{js,ts}", "vite.config.ts"],
    languageOptions: { globals: { ...globals.node } },
  },
);
