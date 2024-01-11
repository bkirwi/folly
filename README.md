[![rm1](https://img.shields.io/badge/rM1-supported-green)](https://remarkable.com/store/remarkable)
[![rm2](https://img.shields.io/badge/rM2-supported-green)](https://remarkable.com/store/remarkable-2)
[![opkg](https://img.shields.io/badge/OPKG-folly-blue)](https://toltec-dev.org/)
[![Discord](https://img.shields.io/discord/385916768696139794.svg?label=reMarkable&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/ATqQGfu)

# Folly

A Z-machine for the reMarkable tablet.

![image](https://user-images.githubusercontent.com/1596339/143691049-aa4bb8aa-e5d4-4247-bdcb-2905e478bd25.png)


**This software is in alpha.**
If you encounter any bugs,
[please report them](https://github.com/bkirwi/armrest/issues)!

# Interactive fiction

The Z-machine is a virtual machine designed specifically
for [interative fiction](https://en.wikipedia.org/wiki/Interactive_fiction).
_Folly_ can run (almost) any of the [hundreds of stories](https://ifdb.org/search?searchfor=format%3AZ*&searchgo=Search+Games)
that have been released in the Z-machine format,
but adapts them to accept handwritten commands instead of keyboard input.
This makes them play well on a device like the reMarkable,
where typing is annoying but reading and writing is very natural.

_Folly_ is built on top of [Encrusted](./encrusted-heart) --
a Z-machine implementation for the Rust programming language --
and [Armrest](https://github.com/bkirwi/armrest),
which provides the handwriting recognition and the building blocks of the UI.

# About the handwriting recognition

_Folly_ uses the open-source handwriting recognition from `armrest`.
It was created from scratch for the tablet,
and runs locally on the device.
When an application needs input from you,
you'll see a prompt like `>_____` onscreen;
just write out the command you want on the line,
and it'll be interpreted and run automatically within a second or so.

The handwriting recognition is not perfectly reliable,
and you may need to repeat an input to get Folly to understand it.
Some advice on getting the best results:
- Make sure you know [the standard IF commands](http://pr-if.org/doc/play-if-card/play-if-card.html)!
  (The handwriting recognition is tuned to recognize the words your game actually uses.)
- Write in lowercase: no capital letters.
- Printing is more reliably recognized than cursive.
- If you can't get the game to recognize a word, try a synonym.

If the handwriting input gets frustrating
(it's particularly bad at recognizing numbers at the moment)
or you're not sure how to input some special character,
there's also an on-screen keyboard available:
tap the little keyboard icon next to the prompt to bring it up.

The game logs your handwriting input,
and its best guess at the corresponding text,
to the `ink.log` file in `FOLLY_ROOT`.
(See below for more on that directory.)
**Please consider contributing this data to the project,
especially if the handwriting recognition is not working well for you**...
it will help us improve the system,
for you and for anyone else with a similar handwriting style.
You can submit the data by [creating an issue](https://github.com/bkirwi/armrest/issues/new)
and adding the `ink.log` file from your device as an attachment.

# Running on reMarkable

Folly is available as the `folly` package in [Toltec](https://toltec-dev.org/),
a community-managed package repository.
This is by far the simplest way to install Folly on your device.
See the Toltec documentation for instructions
on how to get set up and ready to install packages.
(And don't forget to write down your password!)

You'll also need a game to play.
You can find freely available games [on the IFDB](https://ifdb.org/search?searchfor=format%3AZ*&searchgo=Search+Games).
Any game with a `.z3`, `.z4`, `.z5`, or `z8` extension is expected to work.
By default, the app will look for game files under `/home/root/.local/share/folly`.
[Emily Short's Bronze](https://ifdb.org/viewgame?id=9p8kh3im2j9h2881) is packaged with the app,
as well as a short tutorial intended to help you get used to playing games on your tablet.

## Manual setup

While installing via Toltec is recommended,
it's also possible to install Folly manually.
First, you'll need a copy of the binary.
You can either build it from scratch (see instructions below)
or grab a prebuilt binary from the [releases page](https://github.com/bkirwi/encrusted/releases).

The `FOLLY_ROOT` environment variable sets the directory
where _Folly_ will look for game files, store saved games, and keep logs.
It defaults to `/home/root/.local/share/folly`.
(Make sure you don't put the binary at that path!
I suggest `/home/root/bin/folly` instead.)

If you're using a launcher, you may want to create a draft file for it as well:

```bash
# Create the launcher entry, assuming the binary is at /home/root/bin/folly
cat << EOF > /opt/etc/draft/folly.draft
name=folly
desc=a Z-machine interpreter for the reMarkable
call=/home/root/bin/folly
EOF
```

# Building and development

To build this code,
you'll need to have the `armrest` repo checked out in a sibling directory,
and have a recent reMarkable toolchain installed somewhere.
The usual cargo commands work;
to build for the reMarkable, run `../armrest/build-rm.sh`.

# License
MIT
