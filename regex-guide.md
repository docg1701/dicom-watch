# Regex Guide for DicomWatch

This guide explains how to use regex mode in `filter.pattern`. It assumes no
prior knowledge of regular expressions.

## Quick start: glob vs regex

| Glob pattern     | Equivalent regex     | What it matches                     |
|------------------|----------------------|-------------------------------------|
| `*.zip`          | `.*\\.zip$`          | Any file ending in `.zip`           |
| `exam_*.zip`     | `^exam_.*\\.zip$`    | Files starting with `exam_`         |
| `*.tar.gz`       | `.*\\.tar\\.gz$`     | Files ending in `.tar.gz`           |

**When to use regex**: when glob isn't precise enough. E.g., you only want
files matching `PATIENTID_DATE.zip` and nothing else.

## The basics

A regex (regular expression) is a pattern that describes a set of strings.
DicomWatch uses the [Rust regex crate](https://docs.rs/regex). The pattern
must match the **entire filename** — use `^` and `$` to anchor.

| Symbol | Meaning                                      |
|--------|----------------------------------------------|
| `.`    | Any single character                         |
| `*`    | Zero or more of the previous thing            |
| `+`    | One or more of the previous thing             |
| `?`    | Zero or one of the previous thing             |
| `^`    | Start of the string                          |
| `$`    | End of the string                            |
| `\\.`   | A literal dot (`.` is special, so escape it)  |
| `[abc]`| Any one of a, b, or c                        |
| `[0-9]`| Any digit                                    |
| `[A-Z]`| Any uppercase letter                         |
| `\\d`   | Shortcut for `[0-9]`                         |
| `\\w`   | Shortcut for `[A-Za-z0-9_]`                  |
| `(a|b)`| Either a or b                                |

## Common examples

### Match only .zip files

```
^.*\.zip$
```

- `^` — start of filename
- `.*` — any characters, zero or more
- `\\.` — literal dot
- `zip` — the letters "zip"
- `$` — end of filename

### Match filenames like `CT_2024-01-15.zip` (letters, underscore, date, .zip)

```
^[A-Z]+_\d{4}-\d{2}-\d{2}\.zip$
```

- `[A-Z]+` — one or more uppercase letters
- `_` — literal underscore
- `\\d{4}` — exactly 4 digits (the year)
- `-` — literal hyphen
- `\\d{2}` — exactly 2 digits (month)
- `-` — literal hyphen
- `\\d{2}` — exactly 2 digits (day)
- `\\.zip` — literal ".zip"
- `$` — end

### Match either .zip or .tar.gz

```
^.*\.(zip|tar\.gz)$
```

- `(zip|tar\\.gz)` — either "zip" or "tar.gz"

### Match DICOM-style filenames

Some platforms name files like `STUDY_1.2.840.113619.2.55.3.zip`:

```
^[A-Z]+_[\d.]+\.zip$
```

- `[A-Z]+` — uppercase prefix (e.g., STUDY)
- `_` — underscore
- `[\\d.]+` — one or more digits or dots (the UID)
- `\\.zip$` — .zip extension

## Testing your regex

Before using a regex in DicomWatch, test it! You can use:

1. **Command line** (if you have Rust):
   ```bash
   echo "CT_2024-01-15.zip" | grep -E '^[A-Z]+_\d{4}-\d{2}-\d{2}\.zip$'
   ```

2. **Online tools**: [regex101.com](https://regex101.com) — select "Rust" flavor.

3. **Let DicomWatch validate it**: DicomWatch validates the regex at startup.
   If the regex is invalid, it shows an error message with details.

## Common mistakes

| Mistake                    | Wrong          | Right           |
|----------------------------|----------------|-----------------|
| Unescaped dot              | `file.zip`     | `file\\.zip`     |
| No anchors (matches mid-string) | `[0-9]+`  | `^[0-9]+\\.zip$` |
| Forgetting extension       | `^exam_[0-9]+$`| `^exam_[0-9]+\\.zip$` |
| `*` instead of `.*`       | `exam_*`       | `^exam_.*\\.zip$` |

## Glob vs Regex performance

For simple patterns like `*.zip`, prefer **glob** mode — it's simpler and
faster. Use **regex** only when you need precise matching that glob can't
express.
