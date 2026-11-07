// Generated from the Rust wire types of paddock-assistant (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-assistant --all-features --lib generated.

import type { AiState } from '@decryp7/paddock-ui/shell/pitwall';

export type Role = "user" | "assistant" | "tool";

export type ToolCall = { 
/**
 * Identifies the call within its turn (`call_1`); the result answers it.
 */
id: string, 
/**
 * The tool's name; empty when the call could not be read.
 */
name: string, 
/**
 * The arguments; `{"_invalid_json": "<raw>"}` when the call could not be read.
 */
input: unknown, };

export type Usage = { 
/**
 * Tokens in the prompts.
 */
inputTokens: number, 
/**
 * Tokens generated.
 */
outputTokens: number, };

export type EndReason = "answered" | "cancelled" | "failed" | "budgetToolCalls" | "budgetRounds" | "budgetTokens" | "budgetTime";

export type ErrorKind = "needsAi" | "failed";

export type ContextItem = { 
/**
 * The contribution's id (`pn.selection`).
 */
id: string, 
/**
 * What the chip says.
 */
label: string, 
/**
 * What the model is told; data, never instructions.
 */
text: string, };

export type ToolOutcome = "declined" | "stopped";

export type ApprovalChoice = "allowOnce" | "alwaysAllow" | "deny";

export type Approval = "notAsked" | "once" | "always" | "remembered";

export type StoredError = { 
/**
 * One sentence for the user.
 */
message: string, 
/**
 * For a failure whose cause is the AI engine: the state (the shared component renders it).
 */
needsAi?: AiState, };

export type StoredMessage = { 
/**
 * Who said it.
 */
role: Role, 
/**
 * The text: what the user typed, what the model answered, or what a tool returned to the model.
 */
text: string, 
/**
 * For an assistant message: the tools it called.
 */
toolCalls?: Array<ToolCall>, 
/**
 * For a tool message: the call it answers.
 */
toolCallId?: string, 
/**
 * For a tool message: the tool.
 */
toolName?: string, 
/**
 * For a tool message: the call did not fail.
 */
ok?: boolean, 
/**
 * For a tool message: the card's line.
 */
summary?: string, 
/**
 * For a tool message: the structured result for the app's renderer.
 */
data?: unknown, 
/**
 * For a tool message: the call as the user read it on its card.
 */
callSummary?: string, 
/**
 * For a tool message: the tool changes things.
 */
write?: boolean, 
/**
 * For a tool message: what it changes cannot be undone.
 */
destructive?: boolean, 
/**
 * For a tool message: the user declined the call or stopped the turn (not a failure of the tool).
 */
outcome?: ToolOutcome, 
/**
 * For a tool message of a call that ran: how it was approved.
 */
approval?: Approval, 
/**
 * For the last assistant message of a turn that failed: why.
 */
error?: StoredError, 
/**
 * For the last assistant message of a turn a budget ended: what happened.
 */
endMessage?: string, 
/**
 * For a user message: what was attached.
 */
context?: Array<ContextItem>, 
/**
 * For the last assistant message of a turn: how the turn ended.
 */
end?: EndReason, 
/**
 * When it was said (Unix ms).
 */
atMs: number, };

export type Conversation = { 
/**
 * Its id.
 */
id: string, 
/**
 * The first question, shortened.
 */
title: string, 
/**
 * When it started (Unix ms).
 */
createdMs: number, 
/**
 * When it last changed (Unix ms).
 */
updatedMs: number, 
/**
 * The messages, oldest first.
 */
messages: Array<StoredMessage>, };

export type ConversationSummary = { 
/**
 * Its id.
 */
id: string, 
/**
 * Its title.
 */
title: string, 
/**
 * When it last changed (Unix ms).
 */
updatedMs: number, 
/**
 * How many messages it has.
 */
messageCount: number, };

export type ToolInfo = { 
/**
 * The name.
 */
name: string, 
/**
 * What it does.
 */
description: string, 
/**
 * It changes things (needs the user's approval per call).
 */
write: boolean, 
/**
 * What it changes cannot be undone (implies `write`): its approval is a danger choice.
 */
destructive: boolean, };

export type AssistantInfo = { 
/**
 * The provider's name ("Pitwall (qwen3-8b)").
 */
provider: string, 
/**
 * The tools.
 */
tools: Array<ToolInfo>, 
/**
 * Some tool writes: the composer offers "Allow actions".
 */
hasWrite: boolean, };

export type SendRequest = { 
/**
 * The conversation to continue; none starts a new one.
 */
conversationId?: string, 
/**
 * What the user typed.
 */
text: string, 
/**
 * The context chips still on when the user sent.
 */
context: Array<ContextItem>, 
/**
 * The user ticked "Allow actions": tools that write are offered (each call still needs approval).
 */
allowWrite: boolean, };

export type SendAck = { 
/**
 * The conversation (new or continued).
 */
conversationId: string, };

export type ProviderPhase = { "kind": "queued" } | { "kind": "loading", 
/**
 * Since the load started.
 */
elapsedMs: number, } | { "kind": "generating" };

export type AssistantEvent = { "type": "started", 
/**
 * The conversation's title.
 */
title: string, } | { "type": "phase", 
/**
 * Queued, loading the model (with the time so far) or generating.
 */
phase: ProviderPhase, } | { "type": "delta", 
/**
 * The text.
 */
text: string, } | { "type": "toolStarted", 
/**
 * Answers `ToolFinished` and `ApprovalNeeded`.
 */
callId: string, 
/**
 * The tool's name (empty for a call that could not be read).
 */
name: string, 
/**
 * The call as the user reads it.
 */
summary: string, 
/**
 * The tool changes things.
 */
write: boolean, 
/**
 * What it changes cannot be undone (implies `write`).
 */
destructive: boolean, } | { "type": "approvalNeeded", 
/**
 * The call.
 */
callId: string, 
/**
 * The tool.
 */
name: string, 
/**
 * What it would do.
 */
summary: string, 
/**
 * What it would change cannot be undone: the UI makes Decline the default and Allow a danger button.
 */
destructive: boolean, 
/**
 * The card offers "Always allow" besides Allow once and Deny (an action asks every call, so it never does).
 */
remember: boolean, 
/**
 * The arguments.
 */
input: unknown, } | { "type": "toolFinished", 
/**
 * The call.
 */
callId: string, 
/**
 * The tool.
 */
name: string, 
/**
 * It did not fail.
 */
ok: boolean, 
/**
 * One line for the card.
 */
summary: string, 
/**
 * The structured result for the app's renderer of this tool.
 */
data?: unknown, 
/**
 * Set when the user's choice ended the call: they declined it, or stopped the turn while it waited.
 */
outcome?: ToolOutcome, 
/**
 * How the call was approved, when it ran (none for a call that was refused, declined or stopped).
 */
approval?: Approval, } | { "type": "notice", 
/**
 * The sentence.
 */
message: string, } | { "type": "done", 
/**
 * The final answer (also what was generated until a stop or a budget).
 */
text: string, 
/**
 * Tokens used in the turn.
 */
usage: Usage, 
/**
 * Why it ended.
 */
end: EndReason, 
/**
 * For the budgets: what happened and what to do.
 */
message?: string, } | { "type": "error", 
/**
 * The kind.
 */
kind: ErrorKind, 
/**
 * One sentence for the user.
 */
message: string, 
/**
 * For `needsAi`: the state (the shared component renders it).
 */
needsAi?: AiState, };

export type Envelope = { 
/**
 * The conversation.
 */
conversationId: string, 
/**
 * 1, 2, ... within the turn.
 */
seq: number, } & ({ "type": "started", 
/**
 * The conversation's title.
 */
title: string, } | { "type": "phase", 
/**
 * Queued, loading the model (with the time so far) or generating.
 */
phase: ProviderPhase, } | { "type": "delta", 
/**
 * The text.
 */
text: string, } | { "type": "toolStarted", 
/**
 * Answers `ToolFinished` and `ApprovalNeeded`.
 */
callId: string, 
/**
 * The tool's name (empty for a call that could not be read).
 */
name: string, 
/**
 * The call as the user reads it.
 */
summary: string, 
/**
 * The tool changes things.
 */
write: boolean, 
/**
 * What it changes cannot be undone (implies `write`).
 */
destructive: boolean, } | { "type": "approvalNeeded", 
/**
 * The call.
 */
callId: string, 
/**
 * The tool.
 */
name: string, 
/**
 * What it would do.
 */
summary: string, 
/**
 * What it would change cannot be undone: the UI makes Decline the default and Allow a danger button.
 */
destructive: boolean, 
/**
 * The card offers "Always allow" besides Allow once and Deny (an action asks every call, so it never does).
 */
remember: boolean, 
/**
 * The arguments.
 */
input: unknown, } | { "type": "toolFinished", 
/**
 * The call.
 */
callId: string, 
/**
 * The tool.
 */
name: string, 
/**
 * It did not fail.
 */
ok: boolean, 
/**
 * One line for the card.
 */
summary: string, 
/**
 * The structured result for the app's renderer of this tool.
 */
data?: unknown, 
/**
 * Set when the user's choice ended the call: they declined it, or stopped the turn while it waited.
 */
outcome?: ToolOutcome, 
/**
 * How the call was approved, when it ran (none for a call that was refused, declined or stopped).
 */
approval?: Approval, } | { "type": "notice", 
/**
 * The sentence.
 */
message: string, } | { "type": "done", 
/**
 * The final answer (also what was generated until a stop or a budget).
 */
text: string, 
/**
 * Tokens used in the turn.
 */
usage: Usage, 
/**
 * Why it ended.
 */
end: EndReason, 
/**
 * For the budgets: what happened and what to do.
 */
message?: string, } | { "type": "error", 
/**
 * The kind.
 */
kind: ErrorKind, 
/**
 * One sentence for the user.
 */
message: string, 
/**
 * For `needsAi`: the state (the shared component renders it).
 */
needsAi?: AiState, });

/** Every window hears it while a run goes on; the payload is an `Envelope`. */
export const ASSISTANT_EVENT = 'paddock://assistant';

/** The limits the engine enforces (the composer shows them; the engine checks again). */
export const ASSISTANT_LIMITS = { userChars: 8000, contextItems: 8, contextChars: 4000, conversations: 50, messages: 400 } as const;
