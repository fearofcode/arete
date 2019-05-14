arete
-----

Arete is a simple command-line flashcard application.

It works by importing YAML files containing exercises.

It uses PostgreSQL to store data.

In the future, there may be additional features.

Arete is intended to be used for remembering what you learn about math and science.

## How it works

Arete works by importing YAML files that contain exercises. Right now, each
exercise consists of a description, a source, and a reference answer, all of
which are strings and all of which are required.

You can see a sample of what these files look like <a
href="https://github.com/fearofcode/arete/blob/master/sample_files/valid/thinking_like_a_programmer.yaml">this</a>.

A file can contain any number of exercises.

You then import them by running `arete import <path_to_yaml_file>`.

I run Arete in a separate directory from the code where I write out YAML
files. As I'm reading a book, I'll write down exercises that effectively
summarize what I'm learning.

In this way, Arete helps you remember what you learn in an active way. First,
writing out the exercise gives you an opportunity to restate what you've
learned in your own words, like taking notes.

Second, it provides a way to practice the material. The more active you make
the exercise, the more you get out of it. Once an exercise imported, Arete
tracks when it should be reviewed, calling it "due" when its date to review
has come up. By default, an exercise is due upon creation. If you recall the
answer correctly, the exercise is reviewed again tomorrow. But if you get it
right the next time, the next review date will be two days after, then four,
8, 16, 32, ..., up to a maximum of 6 months between reviews. This idea is
called <a href="https://en.wikipedia.org/wiki/Spaced_repetition">spaced
repetition</a>. By reviewing it at increasingly longer time gaps, you both
use your time more efficiently, and promote long-term memory by recalling
what you've learned at further and further intervals.

Another popular application that does this is <a
href="https://apps.ankiweb.net/">Anki</a>. Anki is more geared toward the
needs of foreign language learners. Its repetition algorithm doesn't really
work as well for Arete, since the exercises intended to be record here are
much more substantial than just recaling a vocabulary word. This is part of
why Arete was created.

Right now, the application asks you to be honest about whether you know the
answer or not. It's up to you to be honest. There's nothing stopping you from
cheating and saying you know the answer when you don't, but that doesn't
benefit you.

## How I use this application

The Arete exercises I create generally take one of two forms: simple
memorization exercises and coding exercises.

The memorization exercises will be something very simple and specific like
"how do you read all lines from a file in Python?".

The code exercises will ask you to actually write code, like "print out the
first 50 Fibonacci numbers".

## How review works

When I review with Arete, I have an editor open with a throwaway file called,
say, `scratch.py` where I write out answers to exercises. Arete initially
just shows you the exercise description and then asks you to attempt it. I go
into my editor, write some code, mark the exercise as "I know it". I write
down a reference solution in the answer field, which I carefully compare what
I wrote to. If I missed an important detail, not to worry: Arete asks you if
your answer matched the reference answer. If I forgot something, I'll update
my answer to say that I didn't know it so I can review it more. The
repetition interval of that exercise will be reset to allow you to practice
more.

Here is a sample of what the review UI looks like when doing an exercise:

<img src="https://raw.githubusercontent.com/fearofcode/arete/master/review_ui.png" alt="Sample Arete review screen.">

The UI is text-based. The top line shows the current exercise being worked on
(including the ID in case you want to edit it later), and how many minutes
have elapsed in the review session. Review sessions are timeboxed to make the
habit of using the app more enjoyable.

At the bottom is an interactive text-based selection widget which responds to
arrow keys and keyboard shortcuts (`y` and `n` in this case).

If you select `Know it`, the referenece answer and source will be displayed.
If your personal answer matches the reference answer, you mark as it correct
again. The exercise gets its update interval doubled (so if you last reviewed
the exercise two days ago and it's due today, you won't have to review it
again for four days) and you move on to the next exercise.

In this way, you efficiently go through your exercises using a very simple,
lightweight UI that is cross-platform. Arete works on Linux, Mac, and
Windows.

I hope you find this application useful. Please <a
href="https://github.com/fearofcode/arete">file an issue</a> if you have any
questions or problems.

## Installation

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

## Setting up PostgreSQL on Linux (e.g., if using on a VPS)

This is mostly a reminder for myself for when setting this up on a VPS.

This is a simple setup where you'll be using the databases with your current
account. Adjust any Postgres setup however you like.

```bash
$ sudo apt update
$ sudo apt install postgresql postgresql-contrib
$ sudo -u postgres createuser --interactive --pwprompt # add yourself as a postgres user
# if you made yourself a superuser, these commands should work
$ createdb -O `whoami` arete
$ createdb -O `whoami` arete_test
```