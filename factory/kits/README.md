# factory/kits

P0 不含任何 PCM / SFZ kit（`synth.waveform` 纯合成）。

P1 起放入 `drum.pcm707style` 等 kit 时，只允许 **原创 / CC0 / 明确可再分发** 的素材（SPEC §1、§9.1）；
**禁止**原厂 ROM / 固件样本。每个文件须在 `factory/licenses/` 登记，并能通过
`agb asset verify --factory` 的许可策略（见 `crates/agb-assets` 中的 `FACTORY_ALLOWED_LICENSES`）。
