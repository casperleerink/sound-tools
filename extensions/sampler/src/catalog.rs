//! The instruments of the library: free libraries on GitHub under CC0 or CC BY, each at a
//! pinned commit, checked against the licence file of each repository. A size is of the files
//! an instrument's SFZ file needs.

use crate::library::{Category, Entry, Library};

const VSCO: Library = Library {
    id: "vsco",
    name: "VSCO 2 Community Edition",
    repository: "sgossner/VSCO-2-CE",
    commit: "6dd651d55dde97fd4028699be9d4481f26917891",
    license: "CC0",
    attribution: None,
};

const KAWAI_UPRIGHT: Library = Library {
    id: "kawai-upright",
    name: "FreePats upright piano KW",
    repository: "freepats/upright-piano-KW",
    commit: "570f6c60ed2eff67accad3b85d5b452e57a3ad28",
    license: "CC0",
    attribution: None,
};

const OLD_UPRIGHT: Library = Library {
    id: "old-upright",
    name: "FreePats piano FB",
    repository: "freepats/old-piano-FB",
    commit: "9707673c65d8a52d7b374af807384005564a89f2",
    license: "CC0",
    attribution: None,
};

const E_PIANOS: Library = Library {
    id: "e-pianos",
    name: "Greg Sullivan E-Pianos",
    repository: "sfzinstruments/GregSullivan.E-Pianos",
    commit: "8c3e581acda3594b553948ff0222d4f84a698376",
    license: "CC BY 3.0",
    attribution: Some("Greg Sullivan (sullivang.net), SFZ by kinwie"),
};

const FM_PIANO: Library = Library {
    id: "fm-piano",
    name: "FreePats FM piano",
    repository: "freepats/fm-piano1",
    commit: "89a92d10b47aea841597408f5cf2e9c8164ecb00",
    license: "CC0",
    attribution: None,
};

const MTG_SAX: Library = Library {
    id: "mtg-sax",
    name: "MTG Solo Saxophones",
    repository: "sfzinstruments/MTG.SoloSax",
    commit: "b494d256549b3d088fdec176ce82867f8a1f58b2",
    license: "CC BY 4.0",
    attribution: Some("MTG (freesound.org/people/MTG), SFZ by kinwie"),
};

const CLASSICAL_GUITAR: Library = Library {
    id: "classical-guitar",
    name: "FreePats Spanish classical guitar",
    repository: "freepats/spanish-classical-guitar",
    commit: "6f4eb1b092acc88f5448cea1a0001bd07b971af8",
    license: "CC0",
    attribution: None,
};

const EMILY_GUITAR: Library = Library {
    id: "emily-guitar",
    name: "Karoryfer Emily guitar",
    repository: "sfzinstruments/karoryfer.emilyguitar",
    commit: "b4920dc662fd9cad6dcaccdeecffdd91c8725d8c",
    license: "CC0",
    attribution: None,
};

const FSBS_GUITAR: Library = Library {
    id: "fsbs-guitar",
    name: "FreePats electric guitar FSBS",
    repository: "freepats/electric-guitar-FSBS-clean",
    commit: "192cf0d9bf2c4ba6ead8e3524ba3f78818e4fe91",
    license: "CC0",
    attribution: None,
};

const YR_BASS: Library = Library {
    id: "yr-bass",
    name: "FreePats electric bass YR",
    repository: "freepats/electric-bass-YR",
    commit: "8dcb7ea9116f417273ef8c030d15e7b3aa654301",
    license: "CC0",
    attribution: None,
};

const SWAGBASS: Library = Library {
    id: "swagbass",
    name: "Karoryfer Swagbass",
    repository: "sfzinstruments/karoryfer.swagbass",
    commit: "9d10fcae71af1975988ddecd5af1c95d372c7355",
    license: "CC0",
    attribution: None,
};

const DOUBLE_BASS: Library = Library {
    id: "double-bass",
    name: "D. Smolken double bass",
    repository: "sfzinstruments/dsmolken.double-bass",
    commit: "c2985eb647109d2a8f30a70071e3e163339d7396",
    license: "CC0",
    attribution: None,
};

const MULDJORD_KIT: Library = Library {
    id: "muldjord-kit",
    name: "FreePats MuldjordKit",
    repository: "freepats/muldjordkit",
    commit: "719fe72bc6693b94f1229674e202881145ab44ed",
    license: "CC BY 4.0",
    attribution: Some("Drum samples provided by DrumGizmo.org, kit by Lars Muldjord"),
};

const SYNTH_PERCUSSION: Library = Library {
    id: "synth-percussion",
    name: "FreePats synthesizer percussion",
    repository: "freepats/synthesizer-percussion",
    commit: "39bbabce8e0e10aa259d5d94fca329f668733185",
    license: "CC0",
    attribution: None,
};

const WORLD_PERCUSSION: Library = Library {
    id: "world-percussion",
    name: "FreePats world percussion",
    repository: "freepats/world-percussion",
    commit: "e54eb2912a0d6d4444ab205d52f778e27da0fc96",
    license: "CC0",
    attribution: None,
};

const SYNTH_BASS: Library = Library {
    id: "synth-bass",
    name: "FreePats synth bass",
    repository: "freepats/synth-bass-1",
    commit: "17095d4d23e960b0566489dc506cc858e3b50e0b",
    license: "CC0",
    attribution: None,
};

const SWEEP_PAD: Library = Library {
    id: "sweep-pad",
    name: "FreePats sweep pad",
    repository: "freepats/sweep-pad",
    commit: "ed3e294fcec8c3ea3ec84471f8c7a75bc09eba04",
    license: "CC0",
    attribution: None,
};

const HEADROOM: Library = Library {
    id: "headroom-piano",
    name: "Headroom Piano",
    repository: "sfzinstruments/BengtNilsson.HeadroomPiano",
    commit: "2a7df3f7252227a3484202c1d61bc1bfe352a971",
    license: "CC BY 4.0",
    attribution: Some("Bengt Nilsson, SFZ by kinwie"),
};

pub const CATALOG: &[Entry] = &[
    Entry {
        id: "piano/grand",
        name: "Grand piano (Headroom)",
        category: Category::Piano,
        library: &HEADROOM,
        sfz: "Headroom Piano.sfz",
        download_bytes: 156_800_000,
    },
    Entry {
        id: "piano/kawai-upright",
        name: "Upright piano (Kawai)",
        category: Category::Piano,
        library: &KAWAI_UPRIGHT,
        sfz: "UprightPianoKW-20220221.sfz",
        download_bytes: 32_800_000,
    },
    Entry {
        id: "piano/old-upright",
        name: "Old upright piano",
        category: Category::Piano,
        library: &OLD_UPRIGHT,
        sfz: "PianoFB 20200401.sfz",
        download_bytes: 34_500_000,
    },
    Entry {
        id: "keys/wurlitzer",
        name: "Wurlitzer electric piano",
        category: Category::Keys,
        library: &E_PIANOS,
        sfz: "Wurlitzer EP200/Wurlitzer EP200.sfz",
        download_bytes: 2_400_000,
    },
    Entry {
        id: "keys/cp80",
        name: "Yamaha CP80 electric grand",
        category: Category::Keys,
        library: &E_PIANOS,
        sfz: "CP80/CP80.sfz",
        download_bytes: 11_000_000,
    },
    Entry {
        id: "keys/fm-piano",
        name: "FM electric piano",
        category: Category::Keys,
        library: &FM_PIANO,
        sfz: "FM-Piano1 20190916.sfz",
        download_bytes: 25_300_000,
    },
    Entry {
        id: "keys/pipe-organ",
        name: "Pipe organ",
        category: Category::Keys,
        library: &VSCO,
        sfz: "OrganLoud.sfz",
        download_bytes: 45_500_000,
    },
    Entry {
        id: "strings/violin-section",
        name: "Violin section, sustain",
        category: Category::Strings,
        library: &VSCO,
        sfz: "ViolinEnsSusVib.sfz",
        download_bytes: 46_000_000,
    },
    Entry {
        id: "strings/viola-section",
        name: "Viola section, sustain",
        category: Category::Strings,
        library: &VSCO,
        sfz: "ViolaEnsSusVib.sfz",
        download_bytes: 71_700_000,
    },
    Entry {
        id: "strings/cello-section",
        name: "Cello section, sustain",
        category: Category::Strings,
        library: &VSCO,
        sfz: "CelloEnsSusVib.sfz",
        download_bytes: 72_600_000,
    },
    Entry {
        id: "strings/double-bass-section",
        name: "Double bass section, sustain",
        category: Category::Strings,
        library: &VSCO,
        sfz: "ContrabassSusVB.sfz",
        download_bytes: 48_000_000,
    },
    Entry {
        id: "strings/violin-section-pizzicato",
        name: "Violin section, pizzicato",
        category: Category::Strings,
        library: &VSCO,
        sfz: "ViolinEnsPizz.sfz",
        download_bytes: 9_000_000,
    },
    Entry {
        id: "strings/solo-violin",
        name: "Solo violin, sustain",
        category: Category::Strings,
        library: &VSCO,
        sfz: "SViolinVib.sfz",
        download_bytes: 74_400_000,
    },
    Entry {
        id: "strings/harp",
        name: "Harp",
        category: Category::Strings,
        library: &VSCO,
        sfz: "Harp.sfz",
        download_bytes: 35_300_000,
    },
    Entry {
        id: "brass/trumpet",
        name: "Trumpet, sustain",
        category: Category::Brass,
        library: &VSCO,
        sfz: "TrumpetSus.sfz",
        download_bytes: 40_500_000,
    },
    Entry {
        id: "brass/french-horn",
        name: "French horn, sustain",
        category: Category::Brass,
        library: &VSCO,
        sfz: "FHornSus.sfz",
        download_bytes: 50_600_000,
    },
    Entry {
        id: "brass/trombone",
        name: "Trombone, sustain",
        category: Category::Brass,
        library: &VSCO,
        sfz: "TromboneSus.sfz",
        download_bytes: 60_100_000,
    },
    Entry {
        id: "brass/tuba",
        name: "Tuba, sustain",
        category: Category::Brass,
        library: &VSCO,
        sfz: "TubaSus.sfz",
        download_bytes: 31_200_000,
    },
    Entry {
        id: "woodwinds/flute",
        name: "Flute, sustain",
        category: Category::Woodwinds,
        library: &VSCO,
        sfz: "FluteSusVib.sfz",
        download_bytes: 25_200_000,
    },
    Entry {
        id: "woodwinds/oboe",
        name: "Oboe, sustain",
        category: Category::Woodwinds,
        library: &VSCO,
        sfz: "OboeSusVib.sfz",
        download_bytes: 24_000_000,
    },
    Entry {
        id: "woodwinds/clarinet",
        name: "Clarinet, sustain",
        category: Category::Woodwinds,
        library: &VSCO,
        sfz: "ClarinetSus.sfz",
        download_bytes: 59_400_000,
    },
    Entry {
        id: "woodwinds/bassoon",
        name: "Bassoon, sustain",
        category: Category::Woodwinds,
        library: &VSCO,
        sfz: "BassoonSus.sfz",
        download_bytes: 35_200_000,
    },
    Entry {
        id: "woodwinds/alto-sax",
        name: "Alto sax",
        category: Category::Woodwinds,
        library: &MTG_SAX,
        sfz: "MTG Solo Saxophones/MTG Alto Sax (NL).sfz",
        download_bytes: 29_000_000,
    },
    Entry {
        id: "woodwinds/tenor-sax",
        name: "Tenor sax",
        category: Category::Woodwinds,
        library: &MTG_SAX,
        sfz: "MTG Solo Saxophones/MTG Tenor Sax (NL).sfz",
        download_bytes: 28_400_000,
    },
    Entry {
        id: "guitar/classical",
        name: "Classical guitar (nylon)",
        category: Category::Guitar,
        library: &CLASSICAL_GUITAR,
        sfz: "SpanishClassicalGuitar-20190618.sfz",
        download_bytes: 5_300_000,
    },
    Entry {
        id: "guitar/electric",
        name: "Electric guitar, clean",
        category: Category::Guitar,
        library: &EMILY_GUITAR,
        sfz: "emily_clean.sfz",
        download_bytes: 123_400_000,
    },
    Entry {
        id: "guitar/electric-light",
        name: "Electric guitar, clean (light)",
        category: Category::Guitar,
        library: &FSBS_GUITAR,
        sfz: "EGuitarFSBS-clean bridge small 20260807.sfz",
        download_bytes: 3_100_000,
    },
    Entry {
        id: "bass/electric-finger",
        name: "Electric bass, finger",
        category: Category::Bass,
        library: &YR_BASS,
        sfz: "FingerBassYR 20190930.sfz",
        download_bytes: 3_300_000,
    },
    Entry {
        id: "bass/electric",
        name: "Electric bass",
        category: Category::Bass,
        library: &SWAGBASS,
        sfz: "swagbass_shiny.sfz",
        download_bytes: 109_700_000,
    },
    Entry {
        id: "bass/double-bass-pizzicato",
        name: "Double bass, pizzicato",
        category: Category::Bass,
        library: &DOUBLE_BASS,
        sfz: "d_smolken_rubner_bass_pizz.sfz",
        download_bytes: 136_800_000,
    },
    Entry {
        id: "drums/acoustic-kit",
        name: "Acoustic drum kit",
        category: Category::Drums,
        library: &MULDJORD_KIT,
        sfz: "MuldjordKit 20201018.sfz",
        download_bytes: 138_900_000,
    },
    Entry {
        id: "drums/orchestral-percussion",
        name: "Orchestral percussion kit (GM layout)",
        category: Category::Drums,
        library: &VSCO,
        sfz: "GM-StylePerc.sfz",
        download_bytes: 154_700_000,
    },
    Entry {
        id: "drums/synth-kit",
        name: "Vintage synth drum kit",
        category: Category::Drums,
        library: &SYNTH_PERCUSSION,
        sfz: "SynthesizerPercussion-20220718.sfz",
        download_bytes: 1_500_000,
    },
    Entry {
        id: "percussion/timpani",
        name: "Timpani",
        category: Category::Percussion,
        library: &VSCO,
        sfz: "Timpani.sfz",
        download_bytes: 36_000_000,
    },
    Entry {
        id: "percussion/glockenspiel",
        name: "Glockenspiel",
        category: Category::Percussion,
        library: &VSCO,
        sfz: "Glockenspiel.sfz",
        download_bytes: 6_400_000,
    },
    Entry {
        id: "percussion/marimba",
        name: "Marimba",
        category: Category::Percussion,
        library: &VSCO,
        sfz: "Marimba.sfz",
        download_bytes: 11_800_000,
    },
    Entry {
        id: "percussion/world",
        name: "World percussion",
        category: Category::Percussion,
        library: &WORLD_PERCUSSION,
        sfz: "WorldPercussion 20200905.sfz",
        download_bytes: 8_200_000,
    },
    Entry {
        id: "synth/bass",
        name: "Synth bass",
        category: Category::Synth,
        library: &SYNTH_BASS,
        sfz: "SynthBass1 20190723.sfz",
        download_bytes: 1_000_000,
    },
    Entry {
        id: "synth/sweep-pad",
        name: "Sweep pad",
        category: Category::Synth,
        library: &SWEEP_PAD,
        sfz: "SweepPad 20190813.sfz",
        download_bytes: 3_100_000,
    },
];
