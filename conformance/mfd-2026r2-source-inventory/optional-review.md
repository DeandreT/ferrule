# What omission means for the 17 revisited inputs

The [slot catalog](optional-inputs.tsv) retains the original 55-slot cohort.
Of the 17 revisited here, five have a language-standard fallback, one has a
bounded no-flags case, three have a joint illustration, and eight still lack
an omitted value. Page numbers below are physical pages of the
[pinned installer manual](context-leaf-review.md).

| Slots | Source and conclusion |
| --- | --- |
| `tokenize-regexp.flags` (504) | The release's regex guide (405, p. 836) illustrates splitting on a literal space with no flags. On p. 838, `s`, `m`, `i`, and `x` enable dot-all, multiline, case-insensitivity and whitespace removal respectively. This specifies opt-in modes and one no-flags result. Topic 504, p. 932 warns of syntax differences in generated C++, C# and Java. |
| `datetime-from-parts.hour, minute` (547) | Optional on p. 961, without literal defaults. On p. 963 the `"0000"` fallback belongs to a UDF string input: substrings actually feed the hour and minute ports. Neither port is omitted. |
| `datetime-from-parts.second, millisecond, timezone` (547) | The p. 963 diagram leaves all three unwired. The documented result is `1999-06-21T09:30:00`. This establishes the rendered result for that combination, not three independent defaults or a UTC offset. |
| `duration-from-parts.hour, minute, second, millisecond, negative` (551) | Pages 965–967 mark these inputs optional, describe the sign when supplied, and illustrate a negative duration with time components. No literal omission rule for the five ports is given. |
| `charset-encode.substitute` (609) | Pages 997–998 restrict this argument to non-Unicode encodings. The Unicode replacement glyph is visible in the table on p. 998; plain text extraction drops it. The UTF-8 example encodes ordinary text and does not exercise replacement. Neither supplies the omitted non-Unicode substitute. |
| XPath 3 `format-date`, `format-dateTime`, `format-time`: `language` (730, 731, 733) | Pages 1067, 1068, 1070 refer to [F&O 3.1 §9.8.4.8](https://www.w3.org/TR/2017/REC-xpath-functions-31-20170321/#lang-cal-place). Omission selects the dynamic context's default language. The actual vendor locale is unknown. |
| XPath 3 `format-integer.language` (732) | Page 1069 identifies F&O 3.1 and describes a supplied language. The [format-integer rule](https://www.w3.org/TR/2017/REC-xpath-functions-31-20170321/#func-format-integer) specifies the dynamic default language when absent. The picture grammar alone does not supply this rule. |
| XSLT 1.0 `document.nodeset` (762) | Page 1082 describes a supplied base node and identifies XSLT 1.0. Topic 1089, p. 1608 names the 16 November 1999 Recommendation. For a string first argument, [§12.1](https://www.w3.org/TR/1999/REC-xslt-19991116#document) defaults the omitted second argument to the stylesheet node containing the expression. Its base resolves the relative URI. The rule does not fix the location of a generated stylesheet. |

The dated W3C Recommendations are external normative sources used with the
release's stated function/language profile. Reviewed HTML SHA-256:
`b5bdcfb2e8b6edbe5a997f8971b0829da124bcd6d071c7f651ee71be99b2a988`
(F&O) and `0f0175f97761e6856b6f53267ef8be6d7875b823f0b807b4fc8fe3adb7907157`
(XSLT). The PDF locator verifier checks vendor text; it does not validate
illustration wiring, external standards or an implementation.
