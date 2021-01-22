# Universal LSIF 
A cli tool for generating LSIF data for any language using a language server.

The current implementation is very naive, but ultimately, it will still be slow.
For a fast but imprecise alternative look at [`lsif-os`](https://github.com/alidn/lsif-os).

## Limitations
It currently only emits data for definitions and references.

## Adding support for a language
Add a new section for the language to `src/language_configs.toml`. Including the `installation_command` is optional, and 
the list of keywords can be empty, but including them will improve performance (by avoiding sending unnecessary requests to the server).

## How it works
It simply traverses a repository, for almost each word, sends a request to the corresponding
language server 
