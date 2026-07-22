---
title: "Built in 13 Messages"
author: "Onur Solmaz <2453968+osolmaz@users.noreply.github.com>"
date: "2026-07-10"
---

# Built in 13 Messages

Fun fact: annotui went from an idea to a tested, merged, and installed Unix tool
in 13 user messages to Codex.

This is the user side of the original build conversation. The messages are shown
as written, including repetitions and typos; assistant updates, tool output, injected
instructions, and transcript compaction copies are omitted. The source Codex session
is `019f4b23-b01a-7623-81e1-ee481a2eadee`.

## 1. The idea

> xxI want to implement a TUI based on Rust, which is a very simple Unix utility. It should take as standard input or through flags some file or buffer and it should spin up a TUI using rata2i.
>
> I should let you comment on the buffer with a user experience of a GitHub PR diff viewer. That is, you use the mouse to select a line range and once you let go of the mouse button it opens up like a text window which you can paste into, which you can type a comment in. You hit Enter and when you do that it puts a comment in there and you should be able to go back and edit those comments. It should roughly have the same user experience one has on the GitHub website but it should be in the terminal. It should show line ranges and such. It just shows comments and it should let you edit the comments and such. The keystrokes should work like any other text editor, like Ctrl-A should go to the beginning and Ctrl-E should go to the end and such.
>
> How would we implement such a thing and how would you name such a utility?

## 2. The references

> take herdr.dev and osolmaz/ghzinga as inspiration

## 3. Build it

> $autoimplement great, create github repo under dutifuldev and implement it along the lines of herdr/ratatui. make sure to test it end to end in tmux or sth.
>
> output should quote like in markdown quotes
>
> it should by default output like
>
> > quoted part line 1
> > quoted part line 2
>
> human comment here ...
>
> so it doesn't output the whole thing. actually, there should be multiple modes, and this should be default, but it should also be possible to output the whole thing with the comments
>
> create docs in the new repo, test it end to end. use slophammer

## 4. Ship in slices

> commit and merge frequently btw, without forgetting to review

## 5. Keep review proportional

> no need to run one more codex review if the last one only surfaced p2

## 6. Run it

> how to run now

## 7. Install it

> install annotui binary to the system

## 8. Improve range selection

> ok good. we don't need to show ctrl+a ctrl+e for line beginning/end
>
> also, shift + up/down buttons should also select, and upon releasing shift, it should enter comment creation
>
> and it should show an indicator for the whole range which the comment is applied on, not just the final run
>
> implement all these, $autoimplement

## 9. Keep the footer simple

> also dont include ctrl+o in there
>
> general basic emacs style shortcuts should hold there. without complicating it
>
> no need to show them in the bar

## 10. Yes, really

> also dont include ctrl+o in there
>
> general basic emacs style shortcuts should hold there. without complicating it
>
> no need to show them in the bar

## 11. One more time

> also dont include ctrl+o in there
>
> general basic emacs style shortcuts should hold there. without complicating it
>
> no need to show them in the bar

## 12. Make comment editing keyboard-only

> hmm ok good. i should be able to edit a comment purely using the keyboard. right now, up right button is missing the comment. $autoimplement , merge autonomously

## 13. What next?

> ok. any followups we should do?

Those messages produced the initial repository and three merged feature changes:
[terminal review comments](https://github.com/osolmaz/annotui/pull/1),
[Shift-based range commenting](https://github.com/osolmaz/annotui/pull/2), and
[keyboard-focused comment editing](https://github.com/osolmaz/annotui/pull/3).
The installed binary and each merge were verified with unit, architecture, mutation,
Slophammer, and tmux end-to-end tests.
