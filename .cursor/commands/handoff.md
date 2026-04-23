# handoff

### Skill: Handoff

**Objective:** Generate a copyable State Manifest for session transition.
**Format:** Markdown code-block.

**Instructions:**
When the user invokes "Handoff", analyze the conversation and output a manifest using this exact structure:

1. **Context:** Project name + Tech stack (e.g., Wails/Go/React).
2. **Milestone:** The overarching goal of this sprint.
3. **Diff:** What was actually changed/built in this specific session.
4. **Queue:** Remaining tasks in order of priority.
5. **Directives:** Any specific patterns (e.g., "Use functional components," "Keep logic in Go services").
