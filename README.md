arete
-----

Arete is a simple command-line flashcard application.

It works by importing YAML files containing exercises.

It uses PostgreSQL to store data.

In the future, there may be additional features.

Arete is intended to be used for remembering what you learn about math and science.

## Setting up PostgreSQL on Linux

This is mostly a reminder for myself for when setting this up on a VPS.

This is a simple setup where you'll be using the databases with your current account. Adjust any Postgres setup however you like.

Linux instructions are provided because it's assumed you will deploy Arete on a Linux VPS.

```bash
$ sudo apt update
$ sudo apt install postgresql postgresql-contrib
$ sudo -u postgres createuser --interactive --pwprompt # add yourself as a postgres user
# if you made yourself a superuser, these commands should work
$ createdb -O `whoami` arete
$ createdb -O `whoami` arete_test
```

## Installation/setup

```bash
$ git clone https://github.com/fearofcode/arete
$ cd arete
$ cargo build --release # this will take a minute or so
$ cargo run --release
```

## And then

- Copy `config.toml.template` to `config.toml`
- Fill in the values appropriately
- Type `arete` for usage
