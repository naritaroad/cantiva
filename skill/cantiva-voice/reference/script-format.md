# Script format

`render_script` parses the script like this:

| Syntax | Meaning |
|---|---|
| Blank line | Ends a line (one paragraph = one take = one WAV file) |
| `@Name：` or `@Name:` at paragraph start | Voice for this line (library name or `engine/speaker`) |
| `{…}` or `｛…｝` right after the voice tag | Tone instruction (Qwen3-TTS built-in voices, CosyVoice3) |
| `# …` at line start | Comment, not spoken |
| No `@` tag | Uses `default_voice` |

An `@` without a colon is spoken as normal text, e.g. `@everyone`.

Files are named `001-…wav`, `002-…wav` in the project folder, and lines appear on Cantiva's timeline in order.

## Example: two-character drama

```
# Scene 1
@旁白：{低沉、緩慢}夜深了，港口只剩下最後一盞燈。

@qwen3-tts/Serena：你還在等那艘船嗎？

@qwen3-tts/Uncle_Fu：嗯。它說好今晚會回來的。
```

## Example: Japanese character lines (GPT-SoVITS library voice)

```
@ボクの声：ボクはね、絶対に負けないんだから！

@ボクの声：……でも、ちょっとだけ休憩してもいいかな。
```

## Tips
- Keep each line under about 200 characters; Cantiva also splits long lines internally at sentence punctuation.
- Tone instructions work best as short descriptors: 「溫柔」「急促、緊張」「像在說悄悄話」.
- For IndexTTS emotion, use `speak` per line with `emotion` or `emotion_vector`; the script `{…}` syntax maps simple emotion words for IndexTTS too.
