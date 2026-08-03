# takumi (匠)

**OpenAPI → typed IR** resolution pipeline.

Lowers OpenAPI specs — parsed into `sekkei`'s types — into resolved, typed
intermediate representations suitable for code generation. `takumi` is the
middle stage: it decides *what* the generators see, so each renderer does not
re-derive the same resolution.

```
sekkei  ->  takumi  ->  the *-forge renderers
```

## Usage

```toml
[dependencies]
takumi = "0.1"
```

## License

MIT — see [LICENSE](./LICENSE).
