# Universal LSIF 
A cli tool for generating LSIF data for any language using a language server.

## Usage
After installing it on your `$PATH` and installing the requierd language server, run it like the following (run --help for more information):

`universal-lsif language-server-name language-name-lowercase path-to-repo`

For TypeScript/JavaScript, you can run the following (note that this client and the server communicate over stdin):

`universal-lsif typescript-language-server --server-args="--stdio" javascript path/to/repo`

For Rust:
`universal-lsif rust-analyzer rust .`

## How it works
It simply traverses a repository, for almost each word, sends a request to the corresponding
language server for finding the definition of every symbol over stdin, and generates the LSIF dump.

## Limitations
It currently only emits data for definitions and references.
Also, The current implementation is very naive, but ultimately, it will still be slow.
For a fast but imprecise alternative look at [`lsif-os`](https://github.com/alidn/lsif-os).

