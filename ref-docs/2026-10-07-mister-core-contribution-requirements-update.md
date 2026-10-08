# MiSTer core contribution page: what changed after 2026-08-23

**Dated supplemental reference, 2026-10-07.** It supersedes the parts of
[`2026-08-23-mister-core-contribution-requirements.md`](2026-08-23-mister-core-contribution-requirements.md)
named below. That file is immutable and stays as written; this one records
what the page says now. (v3.1.0, SUB-1.)

**Primary source:** the MiSTer-devel wiki page *Contributing a Core to MiSTer
FPGA*
(<https://github.com/MiSTer-devel/Wiki_MiSTer/wiki/Contributing-a-Core-to-MiSTer-FPGA>),
read on 2026-10-07 from the wiki's own git repository
(`https://github.com/MiSTer-devel/Wiki_MiSTer.wiki.git`, head `1227b217`,
2026-10-06). The page's latest revision is `317b50e` (2026-09-26). Reading the
repository rather than the rendered page gives the exact text and the date of
every edit. Passages below are quoted from it.

## The revision history since the 2026-08-23 record

The 2026-08-23 record was taken from revision `a6c9017` (2026-08-20). Six
revisions followed:

| revision | date | size of the change | what it did |
| --- | --- | --- | --- |
| `6aaf88a` | 2026-09-19 | 61 lines removed | "Update guidelines": the ten-step page removed |
| `01c1da7` | 2026-09-20 | 84 lines added | the page as it now stands: guidelines, reviewer criteria, FAQ |
| `e634b2e` | 2026-09-20 | 1 line | wording |
| `242277d` | 2026-09-21 | 2 lines | wording |
| `1dfe906` | 2026-09-21 | 1 line | wording |
| `317b50e` | 2026-09-26 | 7 added, 4 removed | the `update_all` FAQ answer |

**Correction to the plan's wording.** `to-dos/mister/IMPLEMENTATION_PLAN.md`
says the page "changed on 2026-09-26". The material rewrite is
`6aaf88a` + `01c1da7`, on 2026-09-19 and 2026-09-20. The 2026-09-26 revision
edits only the FAQ answer about `update_all`: MiSTer-devel goes from "among the
most restrictive paths" to "just one of the different paths", Coin-Op
Collection is added as a contact, and the drop-in database moves into its own
paragraph.

## What the page now asks

The page is scoped to one repository: "This document is to help folks get
their cores ready for submission to MiSTer-devel specifically. Other repos may
have other guidelines."

**Core Submission Guidelines** (numbered on the page):

1. "Your code needs to be published under a compatible open source license
   like **GPLv3** or **MIT**".
2. The template repository structure: `sys/`, `rtl/`, `releases/`, and the
   standard root files (`.qpf`, `.qsf`, `.srf`, `.sdc`, `.sv`, `files.qip`,
   `clean.bat`, `.gitignore`). Unchanged.
3. "Your code should not rely on custom edits to framework files in the `sys`
   folder. Repo admins should be able to maintain your core by dropping in a
   fresh `sys` folder and building an updated binary."
4. The MiSTer development principles and coding guidelines. Unchanged.
5. At least one fully functional compiled release in `releases/` as
   `<core_name>_YYYYMMDD.rbf`. Unchanged.
6. Arcade cores only: MRA files. Unchanged, and not applicable.

**Submission:** "Once your repo is ready, and your code is properly tested,
send a link to your repo to newcores@misterfpga.org. A MiSTer-devel member will
reach out once they have a chance to review the repo."

**What Reviewers Are Looking For** (new). The page calls these "much more
nebulous than the guidelines above":

- "Does the code follow the guidelines above?" A missed guideline goes on a
  list of fixes; "It's the refusal to follow the guidelines that will
  disqualify you."
- "Does the developer understand their code?" The page is explicit that
  prompting an LLM to build a core is fine ("_There is nothing wrong with
  this_"), and equally explicit that "if you're submitting to MiSTer-devel,
  you should understand your code as it is core to the next bullet point."
- "Is the developer willing to maintain their code?" Fixing bugs, responding
  to issues, adding features. Reviewers "will be less inclined to believe that
  this is something you're interested in doing if you are too busy developing
  your next core."
- "Does the developer collaborate well with others?"

**FAQ** (new): a review "can take upwards of a month in some situations";
MiSTer-devel is one route into `update_all` among several (Jotego, Coin-Op
Collection, theypsilon, or a drop-in downloader database); and "Is MiSTer-devel
Anti-AI? Not at all ... as long as a developer understands their code, how
they manifested that code is of little consequence."

## What was removed, against the 2026-08-23 record

| 2026-08-23 record | now |
| --- | --- |
| §1 "The core must demonstrate **preservation value** through accurate implementation of the original system" | **gone**. No accuracy or preservation criterion appears on the page |
| §2 "Fully AI generated code should meet a minimum reasonable bar for readability and include some evidence of quality and accuracy testing" | **gone**, replaced by "Does the developer understand their code?" |
| step 2 "Publish the project as a public GitHub repository" | the word "public" is gone; the reviewer still needs "a link to your repo" |
| §6 steps 3-5: accept the organisation invitation, **transfer the repository**, add it to the Cores list with a unique Home folder | **gone** from the page. What follows a review is "an email back with the decision and next steps" |
| "Your request will be reviewed within a few days" | "upwards of a month in some situations" |

## What this means for RustyNES

These are consequences for the submission case, recorded for the maintainer to
weigh. None of them is a decision.

- **The case was built on a sentence that is no longer on the page.** The
  2026-08-23 record called the AI-code sentence "the single most important
  sentence on the page for this project", because the co-simulation evidence
  answers it directly. The evidence is unchanged and still worth presenting:
  the guidelines ask for code that is "properly tested". It no longer answers
  a stated criterion by name.
- **The new central question is about the maintainer, not the code.**
  "Does the developer understand their code?" and "Is the developer willing
  to maintain their code?" are asked of a person, and no test suite can
  answer them. The sibling's long RTL comments (D14), its per-rung documents
  and its oracle-vs-documentation ledger are evidence a reviewer can use to
  check the answer, but they cannot give it.
- **The duplicate-core risk is now unstated rather than gone.** The page no
  longer mentions preservation value or redundancy. `NES_MiSTer` still
  exists. Whether reviewers weigh that is not written anywhere.
- **The repository must be reachable by the reviewer.** `RustyNES_MiSTer` is
  private today, and the page asks for a link to the repository. Making it
  public is the maintainer's decision and the last step before the email.
- **The repository transfer is no longer described.** Whatever "next steps"
  follow a positive review, the page no longer says the repository moves.
  The 2026-08-23 record's warning about that one-way action now refers to a
  step the page does not describe.
- **Unchanged and already met:** the licence (GPL-3.0-or-later), the template
  layout, an unmodified `sys/` (the sibling vendors it verbatim, checked by
  `tb/check_sys.py`), and the release naming (`RustyNES_YYYYMMDD.rbf`).
