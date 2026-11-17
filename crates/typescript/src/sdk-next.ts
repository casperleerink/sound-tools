// The SDK of the project's own tools. Sound Tools writes this file and `sdk/` when the project
// opens, so an edit there is lost. Read `agent-docs/tools.md` first.
//
// Below is every name a tool may import, by the module of `sdk/` that declares it with its doc.

// the fields of a record and the inputs of a tool
export {
  audio, bang, boolean, choice, file, list, noteFields, notes, number, pattern, text
} from "./sdk/fields";
export type {
  AudioPort, BangInput, BooleanField, ChoiceField, Field, Fields, FileField, Inputs, ListField,
  Note, NumberField, ObjectItem, Outputs, SoundOf, StateOf, TextField, Unit, ValuesOf
} from "./sdk/fields";
// edits of notes, as a piano roll makes them
export {
  humanize, legato, quantize
} from "./sdk/notes";
// signals: what a sound is made of
export {
  abs, adsr, allpass, bandpass, beat, bpm, Buffer, buffer, change, channel, clamp, cos, db, delay,
  exp, Feedback, feedback, floor, graphToJson, highpass, highshelf, hold, input, inputLeft,
  inputRight, log, lookup, lowpass, lowshelf, max, min, mix, mtof, noise, notch, note, output,
  Param, peak, phasor, PI, playing, pow, rise, sampleRate, saturate, saw, Signal, sin, sine,
  Sample, smooth, SoundInput, spectral, sqrt, square, Table, tan, tanh, TAU, triangle, wrap
} from "./sdk/sound";
export type {
  Bin, Bins, Graph, GraphNode, Operand, Stereo, TableRef
} from "./sdk/sound";
// `tool`, its `Context`, children and connections
export {
  hearsNotes, takes, tool
} from "./sdk/tools";
export type {
  Action, At, ChildBang, ChildNotes, ChildNumber, ChildPort, ChildTools, Connection, Context, Declaration,
  DraftChildren, FilledStateOfTool, Frozen, HeardEvent, Key, NewChild, NoteEvent, OwnAudio,
  OwnedChild, OwnNotes, Parent, Part, Performer, ProjectTool, SendTo, StateOfTool, Take, Takes,
  TakingTool, Tick, ToolName, ToolSpec
} from "./sdk/tools";
// the clock, keys and menus
export type {
  ActionKey, Clock, KeyName, MenuItem, MenuOptions, Modifiers, MovedNumber, Offer, Offers,
  PlainKey, Section, TempoMap
} from "./sdk/clock";
// live values: what moves every frame without running a view
export {
  Live
} from "./sdk/values";
export type {
  Channel, Num
} from "./sdk/values";
// a view: its elements, where live values start, `memo`, and the checks a test runs
export {
  act, apply, find, Fragment, h, live, memo, press, render
} from "./sdk/view";
export type {
  Actions, BoxProps, Child, ChildProps, Color, ColorToken, Drawn, GenericCard, Lines, Moved, Node, Op,
  Radius, Rects, Role, Self, Shadow, ShapesProps, Space, TextProps, TextStyle, Tree
} from "./sdk/view";
// what the runtime calls
export {
  host
} from "./sdk/host";
// the records of the built-in tools, from their Rust types
export * from "./sdk/records";
