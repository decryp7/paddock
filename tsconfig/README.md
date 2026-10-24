# @decryp7/paddock-tsconfig

The compiler options the five apps' `app/tsconfig.json` copied (all four identical). An app's `tsconfig.json` is

```json
{ "extends": "@decryp7/paddock-tsconfig/angular.json", "compileOnSave": false, "files": [], "references": [...] }
```

`base.json` is for plain TypeScript packages (scripts, e2e helpers); `angular.json` adds the decorator, module and
Angular compiler options. The repo baseline check (C28) keeps the app's file equal to `templates/repo/app/tsconfig.json`.
