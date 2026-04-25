# Liaozhai MUX 聊齋

*A Rust-based multi-user exegesis for procedural literary fiction and world-building, in the PennMUSH/TinyMUSH lineage*

> **Status:** Early stage. This README is a placeholder; expect everything to change.

---

## About

Liaozhai MUX is a text-world engine bringing together three traditions:

- The graph-based spatial models and live-coding extensibility of classic MUDs (DikuMUD, LPMud, LambdaMOO, PennMUSH, TinyMUSH).
- The data-driven component composition of modern roguelikes (Caves of Qud, Cataclysm DDA, Dwarf Fortress).
- Salience-based procedural text assembly grounded in the Chinese literary commentary tradition — *píngdiǎn* 評點.

The "MUX" stands for **Multi-User eXegesis** — a deliberate echo of the close-reading critical school exemplified by Jin Shengtan, Mao Zonggang, Zhang Zhupo, and Liu Xie's *Wenxin Diaolong* 文心雕龍. The aim is not only to generate narrative but to make narrative *technique* legible as runtime structure: foreshadowing, peripheral description, parallel construction, and other named devices from the *píngdiǎn* corpus become first-class components in the engine.

## Etymology — 聊齋

The project takes its name from *Liáozhāi Zhìyì* 聊齋誌異 ("Strange Tales from a Studio of Leisure"), Pu Songling's late-seventeenth-century collection of nearly five hundred supernatural short stories. Pu Songling framed the work as a scholar-host receiving wandering travelers in his studio and recording the strange tales they brought with them.

That framing — a studio where strange tales accumulate from many sources — is, structurally, what a MUSH is. The name precedes the medium by three centuries.

## Architecture (intent)

The detailed architectural reasoning will live in `docs/architecture.md`. In brief:

- **Server core:** Rust, drawing on PennMUSH/TinyMUSH for the command model, attribute storage, and the soft-code extensibility tradition.
- **World model:** ECS composition (Bevy) layered over MUSH-style attribute-keyed objects.
- **Builder/admin client:** ratatui terminal interface.
- **Text generation:** salience-based procedural assembly, with named narrative-technique components — 草蛇灰線 (*cǎoshé huīxiàn*, "grass-snake ash-line" foreshadowing), 烘雲托月 (*hōngyún tuōyuè*, "painting clouds to set off the moon"), and others — as first-class types.

Everything above is directional. Concrete decisions are still ahead.

## Lineage

This project owes intellectual debt to:

- **PennMUSH**, **TinyMUSH**, **LambdaMOO**, **Evennia** — for the attribute-keyed object model, soft-code extensibility, and typeclass-style flexibility.
- **Caves of Qud** and **Cataclysm DDA** — for ECS-as-content and data-driven architectures.
- **Dwarf Fortress** — for showing what deep simulation looks like underneath narrative surface.
- **Pu Songling** — for the frame story.
- **Jin Shengtan**, **Mao Zonggang**, **Zhang Zhupo**, **Liu Xie** — for the analytical vocabulary.
- **Emily Short** and **Valve's dynamic dialogue research** — for salience-based text assembly.

## Typography

The project uses two registers in its visual identity:

- **聊斋** (Simplified) in **Ma Shan Zheng** (楷書 brush calligraphy) — the gestural logomark, evoking a scholar's hand-written title slip.
- **聊齋** (Traditional) in **Noto Serif TC** (宋體 book printing) — the printed wordmark, used in headers and document type.

The split between Simplified and Traditional forms is intentional, mirroring how the character historically moved between brush and woodblock.

## Hexagram 30 / Cowboys & Beans Games

Liaozhai MUX is part of the **Hexagram 30** collection under **Cowboys & Beans Games**.

## License

Apache Version 2.0
