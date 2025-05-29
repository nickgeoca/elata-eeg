# Notes for AI

*   If the user requests the creation of a new task or note to be tracked, create a new `.md` file in the `todo/` directory. Ensure you also add a corresponding entry or link to this new file in [`todo/README.md`](./todo/README.md:1).

When working on a task within a specific sub-project (e.g., `kiosk/`, `daemon/`, `driver/`), **always start by reviewing the `ai_prompt.md` file within that sub-project's directory.** This document provides crucial context about the project's structure, data flow, and key components, which will help in understanding the task and formulating a plan.

Each sub-project folder (like `daemon/`, `driver/`, `kiosk/`) contains:
* `ai_prompt.md`: For project overview and architectural context. **Consult this first.** It begins with a standard prefix: "this is an architecture doc for the ai to understand the context of this directory rapidly".

There is also a top-level notes file:
* [`ai_scratchpad.md`](./ai_scratchpad.md): For your general temporary notes, thoughts, or debugging information related to any current task across the project. This file can be freely updated or replaced as needed.

Explanation of these files:

`ai_prompt.md` (in sub-project directories like `daemon/`, `driver/`, `kiosk/`)
The goal of this doc is to provide rapid context for an AI (or human) developer to understand the specific sub-project's codebase. It is prefixed with "this is an architecture doc for the ai to understand the context of this directory rapidly". The AI can update this document when changes are made to the codebase, making it easier to maintain and use in the future. Ideally, this document remains concise, with the source code providing the specifics.

[`ai_scratchpad.md`](./ai_scratchpad.md) (at the project root)
The AI can freely add or remove notes to this file. This scratchpad is for general use across any task or sub-project. Feel free to replace its contents as needed. This space is for the AI to keep notes, debug information between coding sessions, or any general thoughts relevant to the current task.
