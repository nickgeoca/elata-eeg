# Notes for AI

*   If the user requests the creation of a new task or note to be tracked, create a new `.md` file in the `todo/` directory. Ensure you also add a corresponding entry or link to this new file in [`todo/README.md`](./todo/README.md:1).

When working on a task within a specific sub-project (e.g., `kiosk/`, `daemon/`), **always start by reviewing the `ai_architecture_prompt.md` file within that sub-project's directory.** This document provides crucial context about the project's structure, data flow, and key components, which will help in understanding the task and formulating a plan.

Each sub-project folder generally contains:
* `ai_architecture_prompt.md`: For project overview and architectural context. **Consult this first.**
* `ai_scratch_pad.md`: For your temporary notes related to the current task.

Explanation of these files:

`ai_architecture_prompt.md`
The goal of this doc is to provide rapid context for an AI (or human) developer to understand the Kiosk application's codebase. The AI can update this document when changes are made to the codebase, making it easier to maintain and use in the future. Ideally, this document remains concise, with the source code providing the specifics.

`ai_scratch_pad.md`
The AI can freely add or remove notes to [`kiosk/ai_scratch_pad.md`](kiosk/ai_scratch_pad.md). Ideally, this scratchpad is used for one issue or task at a time, so feel free to replace the file's contents as needed. This space is for the AI to keep notes, debug information between coding sessions, or any general thoughts relevant to the current task.

