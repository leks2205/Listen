# Languages

Listen's interface is available in 17 languages. This page explains how to switch, how the AI DJ's language fits in, and how to help.

## Changing the interface language

Go to **Settings > General > Application > Language** and pick a language. The change applies after you restart Listen.

The choices are **System default** and these languages, each shown in its own language:

| | | |
| --- | --- | --- |
| English | العربية (Arabic) | বাংলা (Bengali) |
| Deutsch (German) | Español (Spanish) | Français (French) |
| हिन्दी (Hindi) | Italiano (Italian) | 한국어 (Korean) |
| Nederlands (Dutch) | Polski (Polish) | Português (Portuguese) |
| Română (Romanian) | Русский (Russian) | Українська (Ukrainian) |
| اردو (Urdu) | 简体中文 (Simplified Chinese) | |

**System default** (the default) follows your computer's language, read from the `LC_ALL`, `LC_MESSAGES` or `LANG` environment variable on Linux. If your language is not in the list, Listen uses English. Any Chinese locale maps to Simplified Chinese.

Text that has not been translated yet is shown in English.

## The AI DJ's language

The [AI DJ](../dj/ai-dj.md) has its own language setting, so the hosts can speak a different language from the one the interface uses. Find it under **Settings > Playback > AI DJ > Language**. The default, **Same as the app**, follows the interface language. Song titles stay in their own language.

The language of lyrics translation (**Settings > Playback > Lyrics > Translation language**) is a separate setting too.

## Helping to translate

Translations are plain JSON files in the source repository, and corrections or new languages are welcome. See [Translations](../development/translations.md) for the file layout and how to send a change.

## Where the setting is stored

`settings.appearance.language` holds the language code (for example `de` or `zh-Hans`); an empty value means System default. The AI DJ's code is `settings.ai_dj.language`.
