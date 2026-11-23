@echo off
rem Starts the local Ollama server that ChatNoir talks to.
rem The CPU backend is forced: the AVX-tuned one produced garbage text on this machine.
set OLLAMA_LLM_LIBRARY=cpu
start "" "%LOCALAPPDATA%\ChatNoirAI\ollama\ollama.exe" serve
