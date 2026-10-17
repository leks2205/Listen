# Creating a Gemini API key

The [AI DJ](ai-dj.md) talks to Google's Gemini models and needs an API key from you. Listen ships no key and has no server of its own. This page walks you through creating a key for free, putting it into Listen and looking after it.

You need a Google account and you must be 18 or older, because Google's [Gemini API terms](https://ai.google.dev/gemini-api/terms) require it.

> **Note:** Google changes the AI Studio website from time to time. Button names may differ slightly from the ones below.

## Create the key

1. Open [Google AI Studio, API keys](https://aistudio.google.com/apikey) and sign in with your Google account.
2. If this is your first visit, read and accept the terms.
3. Press **Create API key**.
4. Choose a Google Cloud project:
   - If you have never used Google Cloud, pick the option to create the key in a new project. AI Studio creates a project for you, and that is all you need.
   - If you already have projects, you can pick one from the list. Use a project that is only for this purpose if you can, so you can see its usage separately.
5. Copy the key. It usually starts with `AIza`, and newer keys can start with `AQ.`. Treat it like a password.

If AI Studio shows a "default project" or asks you to import one, import or create a project first. A key always belongs to a project.

### The button is greyed out

If **Create API key** cannot be pressed:

- Make sure you have accepted the terms in AI Studio.
- Make sure you are signed in with a personal Google account. Accounts of schools and companies (Google Workspace) can have the Gemini API switched off by an administrator, and you may need to ask them or use a personal account.
- Accounts of people under 18 cannot use the Gemini API.
- The Gemini API is not offered in every country. If it is unavailable where you are, AI Studio says so.
- If no project is listed, create or import one first, then try again.
- Reload the page, or sign out and in again.

## Free tier or billing

You do not need a credit card or any billing to use the AI DJ. The free tier has daily and per-minute limits that are enough for normal listening. The most limited part is the speech: only a small number of spoken breaks per day are included. When it runs out, the DJ shows its lines as text. See **Quotas and the text fallback** in [AI DJ](ai-dj.md#quotas-and-the-text-fallback). Google decides the exact limits and may change them; AI Studio shows the current ones for your project.

Free-tier requests differ from paid ones in how Google handles your data. Outside the EU, UK and Switzerland, Google may use free-tier requests to improve its products and human reviewers may read them. Paid usage (billing enabled on the key's project) is not used that way. The AI DJ sends song titles, artists, the DJ's lines and your **Details for the DJ**, so decide whether you are comfortable with this. [What is sent](ai-dj.md#privacy-and-data-sent-to-google) is listed on the AI DJ page.

If you enable billing to get higher limits:

1. In the Google Cloud console open **Billing**, then **Budgets and alerts**, and create a budget for the project with an amount you are comfortable with, for example 5 euros.
2. Add e-mail alerts at a few percentages of the budget.

A budget alert warns you, but it does not stop the spending by itself. The AI DJ uses small text and speech requests, so normal use costs little, but a key that someone else gets hold of can cost money.

## Put the key into Listen

1. Open **Settings → Playback → AI DJ**.
2. Click into **Gemini API key** and paste the key.
3. Press **Save** (or Enter). Listen stores the key in your system keyring and plays a short voice sample to test it.
4. If the status line says that the persona works, switch **Enabled** on.

The key is never shown again; the field then shows dots. It is not written to the settings file.

## Replace, rotate or remove the key

- **Replace or rotate.** Create a new key in AI Studio, paste it into **Gemini API key** and press **Save**. Then delete the old key in AI Studio. Saving a new key also clears a "quota used up" state, which is useful if the new key is on a different project.
- **Remove.** Clear the field and press **Save** with the field empty. Listen deletes the key from the keyring and switches the AI DJ off.
- **Revoke at Google.** Delete the key in [AI Studio](https://aistudio.google.com/apikey). Listen then shows a "key was rejected" warning until you save a working key.
- **Restrict.** In the Google Cloud console you can limit a key to the Generative Language API, which is the one Listen uses.

If your key leaks, for example because you posted a screenshot, delete it at once in AI Studio and create a new one.

## Troubleshooting

- **"The Gemini API key was rejected".** The key was typed or pasted incompletely, has been deleted, or is restricted to something else. Copy it again and save.
- **A quota message.** The free limits are used up. See [AI DJ](ai-dj.md#quotas-and-the-text-fallback).
- **No keyring on your system.** The key cannot be stored permanently. On Linux you need a running secret service such as GNOME Keyring or KWallet. See [Troubleshooting](../more/troubleshooting.md).
- **Testing without the keyring.** The environment variable `LISTEN_GEMINI_API_KEY` is used before the saved key.
