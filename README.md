arete
-----

Arete is a simple command-line flashcard application.

It works by importing YAML files containing exercises.

It uses PostgreSQL to store data.

In the future, there may be additional features.

Arete is intended to be used for remembering what you learn about math and science.

I'm happy to add more documentation and instructions if there is interest. For now, this is probably only going to be used by its creator.

I've experimented with many different languages and frameworks to implement this product in. All of
them have serious drawbacks, so for now, I'm going command-line only, which makes things like
displaying images impossible, and makes things like editing multi-line values awkward.

To be honest, I just made this public so I have something to pin other than my old Java stock prediction
app that never really worked and has horrible code I'm incredibly embarrassed by.

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
