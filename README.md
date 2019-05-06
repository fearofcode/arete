arete
-----

flashcard WIP thing

## Setting up PostgreSQL on Linux

To use Arete, you'll first have to install PostgreSQL if you don't already have it installed.

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

## And then

- Copy `config.toml.template` to `config.toml`
- Fill in the values appropriately
- Build the app, run `bootstrap_schema`, then `import` files, then `review` them