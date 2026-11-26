//! Hand-written conformance cases: each is a sequence of feeds and resizes on a small screen.

use crate::engine::Step;

pub struct Case {
    pub name: String,
    pub cols: usize,
    pub rows: usize,
    pub steps: Vec<Step>,
}

fn feed(s: &str) -> Step {
    Step::Feed(s.as_bytes().to_vec())
}

fn case(name: &str, cols: usize, rows: usize, input: &[&str]) -> Case {
    Case {
        name: name.into(),
        cols,
        rows,
        steps: input.iter().map(|s| feed(s)).collect(),
    }
}

fn raw(name: &str, cols: usize, rows: usize, input: &[&[u8]]) -> Case {
    Case {
        name: name.into(),
        cols,
        rows,
        steps: input.iter().map(|b| Step::Feed(b.to_vec())).collect(),
    }
}

fn numbered(rows: usize) -> String {
    (1..=rows)
        .map(|i| format!("row{i:02}"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

pub fn all() -> Vec<Case> {
    let fill = numbered(6);
    let mut v = vec![
        case("print/plain", 20, 4, &["hello\r\nworld"]),
        case("print/autowrap", 10, 4, &["0123456789ABC"]),
        case("print/wrap_pending_then_cr", 10, 4, &["0123456789\rX"]),
        case("print/wrap_pending_then_cub", 10, 4, &["0123456789\x1b[DX"]),
        case(
            "print/autowrap_off",
            10,
            4,
            &["\x1b[?7l0123456789ABC\x1b[?7hZ"],
        ),
        case("print/bs_at_col0", 10, 4, &["\x08\x08A"]),
        case("print/bs_after_wrap", 5, 3, &["abcde\x08X"]),
        case("print/tabs", 40, 3, &["a\tb\t\tc"]),
        case("print/tab_past_end", 12, 3, &["\t\t\t\tX"]),
        case(
            "print/hts_tbc",
            40,
            3,
            &["\x1b[3g\x1b[1;5H\x1bH\x1b[1;1H\tA\x1b[1;5H\x1b[0g\x1b[1;1H\tB"],
        ),
        case("print/cht_cbt", 40, 3, &["\x1b[2IA\x1b[1ZB"]),
        case("print/rep", 20, 3, &["ab\x1b[5bX"]),
        case("print/lf_vs_crlf", 20, 4, &["a\nb\nc"]),
        case("print/vt_ff_as_lf", 20, 4, &["a\x0bb\x0cc"]),
        case("print/nul_ignored", 20, 2, &["a\0b"]),
        case("print/del_ignored", 20, 2, &["a\x7fb"]),
        case("print/bell_ignored", 20, 2, &["a\x07b"]),
        case("print/utf8_2byte", 20, 2, &["caf\u{e9} \u{3b1}\u{3b2}"]),
        case("cursor/cup", 20, 6, &["\x1b[3;7HX"]),
        case("cursor/cup_clamped", 20, 6, &["\x1b[99;99HX"]),
        case("cursor/cup_zero_params", 20, 6, &["\x1b[0;0HX"]),
        case("cursor/hvp", 20, 6, &["\x1b[2;3fX"]),
        case(
            "cursor/cuu_cud_cuf_cub",
            20,
            6,
            &["\x1b[3;3H\x1b[2A\x1b[3C\x1b[1B\x1b[2DX"],
        ),
        case(
            "cursor/cuu_stops_at_margin",
            20,
            6,
            &["\x1b[3;5r\x1b[4;1H\x1b[9AX"],
        ),
        case(
            "cursor/cud_stops_at_margin",
            20,
            6,
            &["\x1b[2;4r\x1b[3;1H\x1b[9BX"],
        ),
        case("cursor/cnl_cpl", 20, 6, &["\x1b[3;5H\x1b[2EA\x1b[1FB"]),
        case("cursor/cha_vpa_hpa", 20, 6, &["\x1b[8GA\x1b[4dB\x1b[2`C"]),
        case(
            "cursor/save_restore_decsc",
            20,
            6,
            &["\x1b[2;4H\x1b7\x1b[5;9H\x1b8X"],
        ),
        case(
            "cursor/save_restore_csi",
            20,
            6,
            &["\x1b[2;4H\x1b[s\x1b[5;9H\x1b[uX"],
        ),
        case(
            "cursor/decsc_saves_sgr",
            20,
            3,
            &["\x1b[1;31m\x1b7\x1b[0m\x1b8X"],
        ),
        case(
            "cursor/decsc_saves_wrap_pending",
            5,
            3,
            &["abcde\x1b7\x1b[2;1H\x1b8X"],
        ),
        case(
            "cursor/ind_nel_ri",
            20,
            6,
            &["\x1b[3;1HA\x1bD\x1bEB\x1bM\x1bMC"],
        ),
        case("cursor/decaln", 6, 3, &["\x1b#8"]),
        case(
            "erase/ed0",
            10,
            4,
            &["AAAAAAAAAA\r\nBBBBBBBBBB\r\nCCCCCCCCCC\x1b[2;5H\x1b[J"],
        ),
        case(
            "erase/ed1",
            10,
            4,
            &["AAAAAAAAAA\r\nBBBBBBBBBB\r\nCCCCCCCCCC\x1b[2;5H\x1b[1J"],
        ),
        case(
            "erase/ed2_keeps_cursor",
            10,
            4,
            &["AAAAAAAAAA\r\nBBBB\x1b[2J"],
        ),
        case("print/ht_after_hts", 40, 3, &["\x1b[1;4H\x1bH\x1b[1;1H\tX"]),
        case("print/cht_only", 40, 3, &["\x1b[2IX"]),
        case("print/cbt_only", 40, 3, &["\x1b[1;20H\x1b[ZX"]),
        case("erase/ed3", 10, 3, &["1\r\n2\r\n3\r\n4\r\n5\x1b[3J"]),
        case(
            "erase/el012",
            10,
            4,
            &[
                "AAAAAAAAAA\r\nBBBBBBBBBB\r\nCCCCCCCCCC\x1b[1;5H\x1b[K\x1b[2;5H\x1b[1K\x1b[3;5H\x1b[2K",
            ],
        ),
        case("erase/ech", 10, 2, &["ABCDEFGHIJ\x1b[1;3H\x1b[4X"]),
        case(
            "erase/ech_overflow",
            10,
            2,
            &["ABCDEFGHIJ\x1b[1;8H\x1b[99X"],
        ),
        case("edit/ich", 10, 2, &["ABCDEFGHIJ\x1b[1;3H\x1b[2@"]),
        case("edit/dch", 10, 2, &["ABCDEFGHIJ\x1b[1;3H\x1b[2P"]),
        case("edit/il", 8, 5, &["1\r\n2\r\n3\r\n4\r\n5\x1b[2;1H\x1b[2L"]),
        case("edit/dl", 8, 5, &["1\r\n2\r\n3\r\n4\r\n5\x1b[2;1H\x1b[2M"]),
        case(
            "edit/il_in_region",
            8,
            6,
            &["1\r\n2\r\n3\r\n4\r\n5\r\n6\x1b[2;4r\x1b[3;1H\x1b[L"],
        ),
        case(
            "edit/il_outside_region",
            8,
            6,
            &["1\r\n2\r\n3\r\n4\r\n5\r\n6\x1b[2;4r\x1b[6;1H\x1b[L"],
        ),
        case("edit/irm", 10, 2, &["ABCDEF\x1b[1;3H\x1b[4hXY\x1b[4lZ"]),
        case("scroll/overflow", 8, 3, &[&fill]),
        case("scroll/su", 8, 5, &["1\r\n2\r\n3\r\n4\r\n5\x1b[2S"]),
        case("scroll/sd", 8, 5, &["1\r\n2\r\n3\r\n4\r\n5\x1b[2T"]),
        case(
            "scroll/region_lf",
            8,
            6,
            &["1\r\n2\r\n3\r\n4\r\n5\r\n6\x1b[2;4r\x1b[4;1H\n\nX"],
        ),
        case(
            "scroll/region_ri",
            8,
            6,
            &["1\r\n2\r\n3\r\n4\r\n5\r\n6\x1b[2;4r\x1b[2;1H\x1bM\x1bMX"],
        ),
        case("scroll/decstbm_homes_cursor", 8, 6, &["\x1b[4;4H\x1b[2;5r"]),
        case("scroll/decstbm_invalid", 8, 6, &["\x1b[5;2r\x1b[6;1H\nX"]),
        case(
            "scroll/region_does_not_add_history",
            8,
            4,
            &["1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[3;1H\n\n\n"],
        ),
        case(
            "scroll/region_top0_adds_history",
            8,
            4,
            &["1\r\n2\r\n3\r\n4\x1b[1;3r\x1b[3;1H\n\n"],
        ),
        case(
            "scroll/su_in_region",
            8,
            6,
            &["1\r\n2\r\n3\r\n4\r\n5\r\n6\x1b[2;4r\x1b[S"],
        ),
        case(
            "scroll/decslrm_scroll",
            10,
            4,
            &[
                "AAAAAAAAAA\r\nBBBBBBBBBB\r\nCCCCCCCCCC\r\nDDDDDDDDDD\x1b[?69h\x1b[3;7s\x1b[2;1H\x1b[L",
            ],
        ),
        case(
            "sgr/basic_attrs",
            20,
            2,
            &["\x1b[1mB\x1b[3mI\x1b[4mU\x1b[7mR\x1b[0mN"],
        ),
        case(
            "sgr/colors_16",
            20,
            2,
            &["\x1b[31mA\x1b[42mB\x1b[91mC\x1b[102mD\x1b[39;49mE"],
        ),
        case("sgr/colors_256", 20, 2, &["\x1b[38;5;200mA\x1b[48;5;17mB"]),
        case(
            "sgr/truecolor_semicolon",
            20,
            2,
            &["\x1b[38;2;1;2;3mA\x1b[48;2;4;5;6mB"],
        ),
        case(
            "sgr/truecolor_colon",
            20,
            2,
            &["\x1b[38:2::1:2:3mA\x1b[48:2::4:5:6mB"],
        ),
        case("sgr/truecolor_colon_nocs", 20, 2, &["\x1b[38:2:1:2:3mA"]),
        case("sgr/256_colon", 20, 2, &["\x1b[38:5:200mA"]),
        case(
            "sgr/partial_reset",
            20,
            2,
            &["\x1b[1;3;4m\x1b[22mA\x1b[23mB\x1b[24mC"],
        ),
        case(
            "sgr/underline_styles",
            20,
            2,
            &["\x1b[4:3mA\x1b[4:0mB\x1b[21mC"],
        ),
        case("sgr/bold_bright_independent", 20, 2, &["\x1b[1;31mA"]),
        case("sgr/bce_el", 10, 2, &["\x1b[44mAB\x1b[K"]),
        case("sgr/bce_ed", 6, 3, &["\x1b[41m\x1b[2J"]),
        case(
            "sgr/bce_ech_dch",
            10,
            2,
            &["\x1b[42mABCDEFGH\x1b[1;3H\x1b[2X\x1b[2P"],
        ),
        case("sgr/bce_il_scroll", 6, 3, &["\x1b[43m1\r\n2\r\n3\r\n4"]),
        case(
            "sgr/malformed_params",
            20,
            2,
            &["\x1b[38;5mA\x1b[38;2;1;2mB\x1b[;;mC"],
        ),
        case(
            "sgr/faint_blink_hidden_strike",
            20,
            2,
            &["\x1b[2mA\x1b[5mB\x1b[8mC\x1b[9mD"],
        ),
        case("sgr/overline_ignored", 20, 2, &["\x1b[53mA"]),
        case("mode/alt_1049", 10, 3, &["main\x1b[?1049hALT\x1b[?1049l"]),
        case("mode/alt_1049_stay", 10, 3, &["main\x1b[?1049hALT"]),
        case("mode/alt_47", 10, 3, &["main\x1b[?47hALT"]),
        case(
            "mode/alt_1047_clear",
            10,
            3,
            &["main\x1b[?1047hALT\x1b[?1047l\x1b[?1047h"],
        ),
        case(
            "mode/alt_cursor_saved",
            10,
            3,
            &["\x1b[2;3Hm\x1b[?1049h\x1b[3;1Hx\x1b[?1049lY"],
        ),
        case(
            "mode/alt_no_history",
            6,
            3,
            &["1\r\n2\r\n3\r\n4\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\x1b[?1049l"],
        ),
        case(
            "mode/decom",
            10,
            6,
            &["\x1b[3;5r\x1b[?6h\x1b[1;1HA\x1b[9;9HB\x1b[?6l\x1b[1;1HC"],
        ),
        case(
            "mode/cursor_visibility_ignored",
            10,
            2,
            &["\x1b[?25lA\x1b[?25hB"],
        ),
        case("mode/ris", 10, 3, &["abc\x1b[31m\x1b[?1049h\x1bcX"]),
        case("mode/decstr", 10, 3, &["\x1b[31mabc\x1b[!pX"]),
        case("mode/lnm", 10, 3, &["\x1b[20ha\nb"]),
        case("charset/dec_graphics", 20, 2, &["\x1b(0lqqk\x1b(B lqk"]),
        case("charset/so_si", 20, 2, &["\x0e lqk\x0f lqk"]),
        case("charset/g1_designate", 20, 2, &["\x1b)0\x0elqk\x0f"]),
        case("charset/uk", 20, 2, &["\x1b(A#\x1b(B#"]),
        case("osc/title_bel", 20, 2, &["a\x1b]0;title\x07b"]),
        case("osc/title_st", 20, 2, &["a\x1b]2;title\x1b\\b"]),
        case(
            "osc/hyperlink",
            20,
            2,
            &["\x1b]8;;http://x\x07link\x1b]8;;\x07 plain"],
        ),
        case("osc/unterminated_then_text", 20, 2, &["a\x1b]0;ti\x18b"]),
        case("dcs/ignored", 20, 2, &["a\x1bP1$rpayload\x1b\\b"]),
        case("dcs/sixel_ignored", 20, 2, &["a\x1bPq#0;2;0;0;0~~~\x1b\\b"]),
        case("apc/ignored", 20, 2, &["a\x1b_Gf=24;AAAA\x1b\\b"]),
        case("esc/can_aborts_csi", 20, 2, &["a\x1b[3\x18b"]),
        case("esc/sub_aborts_csi", 20, 2, &["a\x1b[3\x1ab"]),
        case("esc/esc_inside_csi", 20, 2, &["a\x1b[3\x1b[2;2Hb"]),
        case("esc/c0_inside_csi", 20, 2, &["\x1b[2\nCA"]),
        case("esc/unknown_csi", 20, 2, &["a\x1b[?9999zb\x1b[>cX"]),
        case(
            "esc/long_params",
            20,
            2,
            &[
                "\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18;19;20;21;22;23;24;25;26;27;28;29;30;31;32;33mA",
            ],
        ),
        case("uni/cjk_wide", 20, 2, &["a\u{4e2d}\u{6587}b"]),
        case("uni/cjk_wrap_edge", 5, 3, &["abcd\u{4e2d}e"]),
        case(
            "uni/cjk_overwrite_half",
            10,
            2,
            &["\u{4e2d}\u{6587}\x1b[1;2HX"],
        ),
        case(
            "uni/cjk_overwrite_with_wide_over_narrow",
            10,
            2,
            &["abcd\x1b[1;2H\u{4e2d}"],
        ),
        case("uni/emoji", 20, 2, &["a\u{1f600}b"]),
        case("uni/emoji_zwj", 20, 2, &["a\u{1f468}\u{200d}\u{1f469}b"]),
        case("uni/emoji_vs16", 20, 2, &["a\u{2764}\u{fe0f}b"]),
        case("uni/combining", 20, 2, &["e\u{301}x"]),
        case("uni/combining_at_col0", 20, 2, &["\u{301}x"]),
        case("uni/zero_width_space", 20, 2, &["a\u{200b}b"]),
        case("uni/fullwidth_latin", 20, 2, &["\u{ff21}\u{ff22}c"]),
        case("uni/ambiguous_width", 20, 2, &["\u{b1}\u{2606}\u{2500}x"]),
        case(
            "uni/dch_over_wide",
            10,
            2,
            &["\u{4e2d}\u{6587}x\x1b[1;1H\x1b[P"],
        ),
        case(
            "uni/el_over_wide",
            10,
            2,
            &["\u{4e2d}\u{6587}x\x1b[1;2H\x1b[K"],
        ),
    ];
    v.extend([
        raw("print/utf8_invalid", 20, 2, &[b"a\xff\xfeb\x80c\xc3(d"]),
        raw("esc/c1_csi_8bit", 20, 3, &[b"a\x9b2;3HX"]),
        raw(
            "uni/utf8_split_across_feeds",
            20,
            2,
            &[&[0xe2], &[0x82], &[0xac, b'x']],
        ),
    ]);

    let resize = |name: &str, cols, rows, pre: &str, to: (usize, usize), post: &str| Case {
        name: name.into(),
        cols,
        rows,
        steps: vec![feed(pre), Step::Resize(to.0, to.1), feed(post)],
    };
    v.extend([
        resize(
            "resize/shrink_rows_pulls_to_history",
            8,
            6,
            &numbered(6),
            (8, 3),
            "",
        ),
        resize(
            "resize/grow_rows_pulls_from_history",
            8,
            3,
            &numbered(6),
            (8, 6),
            "",
        ),
        resize(
            "resize/grow_rows_no_history",
            8,
            6,
            "a\r\nb",
            (8, 9),
            "\r\nc",
        ),
        resize(
            "resize/shrink_cols_truncates_or_reflows",
            10,
            3,
            "0123456789\r\nabc",
            (5, 3),
            "",
        ),
        resize(
            "resize/grow_cols_unwraps",
            5,
            4,
            "0123456789ABC",
            (15, 4),
            "",
        ),
        resize(
            "resize/cursor_after_shrink",
            10,
            6,
            "\x1b[5;5HX",
            (10, 3),
            "Y",
        ),
        resize(
            "resize/alt_screen",
            10,
            5,
            "x\x1b[?1049h\x1b[3;3HALT",
            (6, 3),
            "",
        ),
        resize(
            "resize/region_reset",
            10,
            6,
            "\x1b[2;4r",
            (10, 8),
            "\x1b[8;1H\nX",
        ),
        resize("resize/wrap_pending_cursor", 5, 3, "abcde", (8, 3), "X"),
    ]);
    v
}
