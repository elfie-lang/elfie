# Ideas scratchpad

Loose ideas, not yet decided or implemented.

## Package structure

- Elfie packages are always namespaced.
- Sub packages can be defined by creating an `elfie.json` file in a sub folder and specifying packages.
- The root package can be used as an auto wrapper.
- `def/main.lfy` is the default export unless otherwise specified.

## Compiled packages

- For compiled packages, the config can specify the ecosystem and the package's name in that ecosystem.
- That mapping can be used to auto install across different languages.
