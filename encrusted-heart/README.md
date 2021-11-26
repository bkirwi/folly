# Encrusted Heart

This is a fork of [Sterling DeMille's Encrusted](https://github.com/DeMille/encrusted),
which has been:
- stripped of all UI-specific code (except for a terminal-based example);
- updated to support Z-machine versions 4, 5, and 8;
- modified to expose various bits of state, and simplify the non-blocking input interface;
- debugged;
- brought up to a more modern dialect of Rust; 
- etc.

It omits a number of cool things in the original version,
including some web-specific features like automapping.
See the link above for more.

These changes were made to support the _Folly_ application,
but they ought to be helpful to any other projects who need a Z-machine in Rust.
If you have such a project and need help integrating, please open an issue.

