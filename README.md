## Subject Buffer

### Background

A regular expression will match a _target_ string against a _subject_ string.

```txt
\d+                    <-- TARGET
There are 123 apples.  <-- SUBJECT
```

Regular expression engines can support certain features:

 - `Multi segment matching`. If the subject is very long, then segment by segment can be searched at a time.
 - `Lookbehinds`. Once content is matched, it can be checked that prior characters fulfill a criteria.

Multi segment matching must handle a case where a match straddles the boundary across two segments. This can be handled in one of two ways:

 - The partially matched content can be [retained from the previous segment](https://www.pcre.org/current/doc/html/pcre2partial.html#SEC4) and placed at the beginning of the next segment. <-- this approach is used
 - The match state can be [stored and resumed between segments](https://www.pcre.org/current/doc/html/pcre2partial.html#SEC6).

This case is complicated if lookbehinds are supported as they can straddle segment boundaries as well.

### This Lib

This lib manages the subject buffer; when new content is read from a source it retains some content to support lookbehinds and partial matches. Reading new content into the buffer looks like the following:

```txt
1: characters already processed
2: lookbehind characters to retain (in this case 5 characters)
3: the match offset and proceeding bytes (partial or no match)
4: newly filled range

1          2    3
-----------12345xxxxxxxx
↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓↓
2    3       4
12345xxxxxxxxNEWNEWNEWNE

2 and 3 are moved to the beginning of the buffer, discarding 1.
4 is newly available, and filled with the next content from the subject.
```

### Details

A key feature of this lib is that the buffer will ALWAYS have enough proceeding bytes to satisfy the lookbehind - a bound check is not required. If at the beginning of the source, then the buffer is padded with null bytes to make this true. This simplifies pattern matching at the cost of a small overhead in verifying that the match's lookbehind doesn't contain this padding (a function is provided for this check).

For example, with a lookbehind of 3 bytes, the content after an initial read will look like this:

```txt
   match offset
   V
000content here...
```

