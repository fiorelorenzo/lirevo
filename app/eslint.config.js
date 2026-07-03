import js from "@eslint/js";
import ts from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import globals from "globals";
import prettier from "eslint-config-prettier";
import svelteConfig from "./svelte.config.js";

export default ts.config(
  {
    ignores: ["build/", "dist/", ".svelte-kit/", "node_modules/", "src-tauri/"],
  },
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs.recommended,
  prettier,
  ...svelte.configs.prettier,
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
      // lirevo is a Tauri SPA (no base path) and vendors shadcn-svelte
      // components; SvelteKit's resolve()-for-navigation adds nothing here.
      "svelte/no-navigation-without-resolve": "off",
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts", "**/*.svelte.js"],
    languageOptions: {
      parserOptions: {
        parser: ts.parser,
        svelteConfig,
      },
    },
    rules: {
      // $bindable() prop defaults read as "useless" to the rule's data-flow
      // analysis (the value flows to the parent binding, not a local read).
      "no-useless-assignment": "off",
    },
  },
);
