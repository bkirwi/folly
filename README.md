# Encrusted (reMarkable version)

### A z-machine (interpreter) for Infocom-era text adventure games like Zork

https://user-images.githubusercontent.com/1596339/125673318-b95d00c0-ed43-4360-87ea-3e115d5a6876.mov

**This is a fork of the [Encrusted interpreter by @DeMille](https://github.com/DeMille/encrusted),
rebuilt for the reMarkable tablet using
[`armrest`](https://github.com/bkirwi/armrest).**
This readme describes the fork -
follow the links above for more context.

**This software is in alpha.**
If you encounter any bugs,
[please report them](https://github.com/bkirwi/armrest/issues)!

# About the handwriting recognition

Encrusted uses the open-source handwriting recognition from `armrest`,
which was created from scratch for the tablet.
It runs fully locally on the device...
but is not yet as reliable as the "cloud" handwriting recognition
for documents in the main reMarkable application.

You will occasionally need to repeat an input to get Encrusted to understand it.
Some advice on getting the best results:
- Make sure you know [the standard IF commands](http://pr-if.org/doc/play-if-card/play-if-card.html).
  (The handwriting recognition is tuned to best recognize the words
  that the game you're playing expects!)
- Write in lowercase: no capital letters.
- Printing is more reliably recognized than cursive at the moment.
- If you can't get the game to recognize a word, try a synonym.

The game logs your handwriting input,
and its best guess at the corresponding text,
to the `ink.log` file in `ENCRUSTED_ROOT`.
Please consider contributing this data to the project,
especially if the handwriting recognition was not working well for you...
this sort of training data is extremely valuable
for improving the quality of the recognizer.
(You can submit the data by [creating an issue](https://github.com/bkirwi/armrest/issues/new)
and adding the `ink.log` file from your device as an attachment.)

# Running on reMarkable

First, you'll need a copy of the binary.
You can either build it from scratch (see instructions below)
or grab a prebuilt binary from the [releases page](https://github.com/bkirwi/encrusted/releases).

You'll also need a game to play. Freely available games include:
- [Minizork](https://github.com/bkirwi/encrusted/raw/master/tests/minizork.z3) -
  a pared-down version of the classic [Zork](https://en.wikipedia.org/wiki/Zork).
- [Hitchhiker's Guide to the Galaxy](http://www.douglasadams.com/creations/hhgg.z3) -
  a clever (but very difficult) game
  [based on the beloved novel](https://en.wikipedia.org/wiki/The_Hitchhiker%27s_Guide_to_the_Galaxy_(video_game))

(But any Infocom-era game with a `.z3` extension is expected to work.)


The `ENCRUSTED_ROOT` environment variable sets the directory
where Encrusted will look for game files, store saved games, and keep logs.
It defaults to `/home/root/encrusted`.
(Make sure you don't put the binary at that path!
Consider `/home/root/bin/encrusted` instead.)

If you're using a launcher, you may want to create a draft file for it as well:

```bash
# Create the launcher entry, assuming the binary is at /home/root/bin/encrusted
cat << EOF > /opt/etc/draft/encrusted
name=encrusted
desc=a Z-machine interpreter for the reMarkable
call=/home/root/bin/encrusted
EOF
```

# Building and development

To build this code,
you'll need to have the `armrest` repo checked out in a sibling directory.
To build for the reMarkable, run `../armrest/build-rm.sh`.

### Tests

Run z-machine tests ([czech](https://inform-fiction.org/zmachine/standards/z1point1/appc.html) & [praxix](https://inform-fiction.org/zmachine/standards/z1point1/appc.html)) through [regtest](https://eblong.com/zarf/plotex/regtest.html):
```
npm run test
```

### Notes
- Currently only supports v3 zcode files
- Saves games in the Quetzal format

### License
MIT
