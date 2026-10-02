// @ts-check
const eslint = require('@eslint/js');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');
const prettier = require('eslint-config-prettier/flat');

module.exports = defineConfig([
  {
    files: ['**/*.ts'],
    extends: [
      eslint.configs.recommended,
      tseslint.configs.recommended,
      tseslint.configs.stylistic,
      angular.configs.tsRecommended,
    ],
    processor: angular.processInlineTemplates,
    rules: {
      '@angular-eslint/directive-selector': [
        'error',
        {
          type: 'attribute',
          prefix: 'app',
          style: 'camelCase',
        },
      ],
      '@angular-eslint/component-selector': [
        'error',
        {
          type: 'element',
          prefix: 'app',
          style: 'kebab-case',
        },
      ],
      // AGENTS.md, "Named types"
      'no-restricted-syntax': [
        'error',
        {
          selector: ':not(TSTypeAliasDeclaration) > TSTypeLiteral',
          message:
            'Name this object type: an interface or a type alias (AGENTS.md, "Named types").',
        },
        {
          selector: ':not(TSTypeAliasDeclaration) > TSTupleType',
          message: 'Name this tuple type with a type alias (AGENTS.md, "Named types").',
        },
      ],
      // AGENTS.md, "Angular for speed": preferences, so warnings
      'no-restricted-imports': [
        'warn',
        {
          paths: [
            {
              name: '@angular/forms',
              message: 'Prefer signal forms from @angular/forms/signals.',
            },
            {
              name: '@angular/common/http',
              message: 'Prefer resource() with fetch (api.ts).',
            },
          ],
        },
      ],
    },
  },
  {
    files: ['**/*.html'],
    extends: [angular.configs.templateRecommended, angular.configs.templateAccessibility],
    rules: {},
  },
  prettier,
]);
