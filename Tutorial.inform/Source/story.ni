"Welcome to Folly" by "Ben Kirwin"

The story headline is "An Interactive Tutorial".
The story genre is "educational".
The release number is 1.
The story description is "An introduction to Folly, the handwritten Z-machine for the reMarkable tablet."
The story creation year is 2022.

Include Basic Screen Effects by Emily Short.
Include Basic Help Menu by Emily Short.

[
Things to teach the user:
- Swipe left and right to turn the page
- Write on the line to provide inputs normally
- Tap 'space' when the game says to press space or any key
- Open the keyboard 
- Save and restore
]

To say Folly:
 	say "Folly"

When play begins:
	Say "Welcome to [Folly] - an interactive fiction interpreter for the reMarkable tablet.[paragraph break]The Z-machine was designed by Infocom in the 1970s, designed specifically for interactive fiction games. Thousands of games have been released in this format, and the interpreters you need to play them have been written for almost every sort of computer... including the one in your hands now.[paragraph break]These games were designed to be played sitting at a computer and typing on a keyboard, but you'll be playing them by handwriting on a tablet. Even experienced interactive fiction players may need some help to get used to this! This tutorial is a simple game that should introduce you to [Folly]'s features, along with the basic mechanics of playing a game. Look for the 'tutorial' text in [italic type]italics[roman type]. [paragraph break][italic type]Instead of an infinite scroll of text, [Folly]'s output is broken up into pages. The current page number is on the bottom, and a small '>' next to it means that there are more pages after the current one. Swipe right-to-left when you're ready to flip to the next page, or the other way to flip back.[paragraph break][roman type]"

Table of Things to Do
Status	Introduction	Instruction	Congratulation	Explained
"Command Input"	"[Folly] uses handwriting recognition to understand your commands. It understands most people's handwriting fairly well, but it's not perfect; make sure you print your instructions clearly, ideally in lowercase. [Folly] reads your writing as soon as you lift the pen away... it's normally takes about a second. Let's start by taking that lamp!"	"Write 'get lamp' on the line below, next to the '>' symbol."	"Nice work! If you ever have trouble getting [Folly] to understand your command, you can use the on-screen keyboard: just tap the keyboard icon next to the '>'."	false
"Help"	"Many interactive fiction games contain a help menu. Menus are currently a bit awkward in [Folly], since they were designed for keyboard navigation; you'll need to use the onscreen keyboard for them. This game's help menu contains general instructions for playing IF; you won't need them for this tutorial, but they're helpful information for other games."	"Write 'help' at the prompt, then use the onscreen keyboard to navigate the help menu."	"Great! If you ever want to revisit the help information, just type 'help' at the prompt again."	false
"Saving"	"Along with in-game commands like 'get' and 'look', interactive fictions also support meta-commands like 'save', 'restore', and 'quit'. You run these commands just like any other: by writing them next to the prompt. [Folly] has special support for saving and restoring the game, so let's try those out first."	"Write 'save' at the prompt below."	"Game saved! [Folly] records the contents of the status line along with the save, so you can recognize it later."	false
"Restoring"	"We've saved the game, so let's try restoring it."	"Write 'restore' at the prompt below, then select the game you saved earlier from the list."	"Game restored!"	false
"Quitting"	"The last important meta-command is 'quit', which exits the game and dumps you back to the main menu. If you want to switch games, that's how to do it... but make sure to save first!"	"Write 'quit' at the prompt below."	"That's it! If you're an experienced interactive fiction player, you've learned everything you need to know. If you're still learning, you may want to try a game like Emily Short's [bold type]Bronze[roman type][italic type] with tutorial mode on, to learn more of the usual commands and how the world works."	false


N is a number that varies. N is 1.

Before reading a command:
    If N is greater than the number of rows in the Table of Things to Do, continue the action;
	Choose row N in the Table of Things to Do;
	now the right hand status line is "[Status entry]";
	If Explained entry is false, say "[italic type][Introduction entry][roman type][paragraph break]";
	Now Explained entry is true;
	Say "[italic type][Instruction entry][roman type]".
	
To advance the tutorial:
    If N is greater than the number of rows in the Table of Things to Do, continue the action;
	Choose row N in the Table of Things to Do;
	Say "[italic type][Congratulation entry][roman type][paragraph break]";
	Increase N by 1.
	
The help request rule is not listed in any rulebook.

Carry out asking for help:
	now the current menu is the Table of Instruction Options;
	carry out the displaying activity;
	clear the screen;
	advance the tutorial;
	try looking;
	stop the action.
	
After printing a parser error:
    Say "[line break][italic type]Your command was recognized as [roman type][bold type][the player's command][roman type][italic type]. If that's not what you wrote, that means [Folly][italic type] couldn't understand your handwriting properly. Make sure you're printing your command clearly on the line; unabbreviated, lowercase commands tend to work best. You could also enter the command using the on-screen keyboard... just tap the keyboard icon in the margin, type your command on the keyboard that appears, and hit enter.[paragraph break]".

The description of the player is "Bright-eyed and eager to learn."

Tutorial is a room. "You find yourself in a vast, featureless expanse, stretching as far as you can see in every direction."
 
Instead of going a direction (called way):
	say "You walk [way] for a bit, but seem to end up right where you started."

The brass lamp is here. "A brass lamp lies at your feet." The description is "A handheld brass lantern, lit, and somewhat tarnished by wear." Understand "lantern" as the lamp.

The doorway is nowhere. "You can barely make out a slight contrast in the space around you; a narrow doorway, just ahead." It is an open, enterable container. It is fixed in place. The description is "The space beyond is the same colour as the void around you; if it weren't for the lamplight, you wouldn't have seen it."

Instead of entering the doorway:
	say "You're not ready to leave just yet."

After taking the lamp for the first time:
	Advance the tutorial;
	say "Taken. As you lift the lamp, the light strikes a shape just ahead of you. Your eyes strain...[paragraph break][italic type]Aside from normal, handwritten commands, games may also wait for a single keypress at a time. This is mostly used to ask the user to 'press any key' to pace out long sections of text, like it is just below. In that case, you can just hit the 'space' key that will appear in the middle of your screen.[roman type][paragraph break][bracket]Press any key to continue.[close bracket]";
	wait for any key;
	say "[paragraph break][italic type]Great! Occasionally, you'll need to 'press a key' other than space... when that happens, you can pop up an onscreen keyboard using the icon on the left.[roman type][paragraph break]Squinting in the lamplight, you finally make out the outline of a doorway contrasted against the surrounding void.";
	now the doorway is in the Tutorial.

Report saving the game:
	if N is 3:
		advance the tutorial;
		continue the action.
		
The restore the game rule response (B) is "[post-restore]"

To say post-restore:
	if N is 3:
		now N is 4;
		advance the tutorial;
		say "The surface of the doorway glimmers slightly.";
	otherwise:
		Say "Ok."
	
Carry out quitting the game:
	if N is 5, advance the tutorial;
	continue the action.
		
