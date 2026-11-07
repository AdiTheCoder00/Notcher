# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Windows knowledge workers who want to dictate, ask about on-screen content, or schedule reminders without switching apps.

## Product Purpose

Murmur lets people complete common voice-assistant tasks from their current Windows app. Success means capturing a spoken request and returning its result with minimal interruption to the user's work.

## Positioning

A push-to-talk assistant with no wake word or always-listening microphone. Global keyboard shortcuts start dictation, questions about selected screen content, and voice-scheduled tasks.

## Operating Context

Users hold configurable keyboard chords while speaking. Dictation is pasted into the focused app; questions can include a circled screen region; scheduled tasks are managed in Murmur and run at their requested time. The desktop app also includes a notch, a Control Centre, configurable voice and answer persona, and diagnostics.

## Capabilities and Constraints

- Uses Gemini for speech transcription, screen questions, task scheduling, and task execution.
- Users provide their own Gemini API key; it is stored in Windows Credential Manager.
- Microphone audio and screenshots are held in memory and sent to Gemini when requested; audio is not recorded to disk.
- Scheduled task data and preferences are stored locally in SQLite.
- The app is a Windows desktop app built with Tauri, React, and TypeScript.
- Existing workflows include hold-to-talk dictation, circle-to-ask, voice scheduling, configurable shortcuts, and the notch Control Centre.

## Brand Commitments

- Product name: Murmur.
- Preserve the existing voice workflows and the push-to-talk, no-wake-word interaction.

## Evidence on Hand

- Product behavior and implementation notes: [README.md](./README.md).
- Working frontend and Windows desktop implementation in `src/` and `src-tauri/`.
- No confirmed testimonials, customer claims, or performance benchmarks. Do not invent them.

## Product Principles

- Keep the user in their current app whenever possible.
- Make microphone and screen capture intentional, user-triggered actions.
- Keep personal configuration and scheduled tasks on the device.
- Report actionable failures instead of hiding them.
