// @ts-check
const eslint = require('@eslint/js');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');
const prettier = require('eslint-config-prettier/flat');
const jsdoc = require('eslint-plugin-jsdoc');

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
    plugins: { jsdoc },
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
      // AGENTS.md, "Readable code": every declaration has a doc comment (classes, methods, functions, interfaces and
      // their members, type aliases, enums and their members, class fields, constants)
      'jsdoc/require-jsdoc': [
        'warn',
        {
          publicOnly: false,
          require: {
            ClassDeclaration: true,
            ClassExpression: true,
            FunctionDeclaration: true,
            MethodDefinition: true,
          },
          contexts: [
            'TSInterfaceDeclaration',
            'TSTypeAliasDeclaration',
            'TSEnumDeclaration',
            'TSEnumMember',
            'TSPropertySignature',
            'TSMethodSignature',
            'TSAbstractMethodDefinition',
            'PropertyDefinition',
            'TSAbstractPropertyDefinition',
            'Program > VariableDeclaration',
            'ExportNamedDeclaration > VariableDeclaration',
          ],
        },
      ],
      // AGENTS.md, "Readable code": warnings, which the refactor clears file by file; new code adds none
      'id-length': ['warn', { min: 2, exceptions: ['i', 'j', 'x', 'y', 'a', 'b'] }],
      'max-lines-per-function': ['warn', { max: 60, skipBlankLines: true, skipComments: true }],
      complexity: ['warn', 15],
      // AGENTS.md, "Angular for speed": preferences, so warnings
      'no-restricted-imports': [
        'warn',
        {
          paths: [
            {
              name: '@angular/forms',
              message: 'Prefer signal forms from @angular/forms/signals.',
            },
          ],
        },
      ],
    },
  },
  {
    // a spec's names say what it checks; the generated types carry the Rust structs' docs (bun run types)
    files: ['**/*.spec.ts', 'src/app/generated/**/*.ts'],
    rules: { 'jsdoc/require-jsdoc': 'off' },
  },
  {
    files: ['**/*.html'],
    extends: [angular.configs.templateRecommended, angular.configs.templateAccessibility],
    rules: {},
  },
  prettier,
]);
