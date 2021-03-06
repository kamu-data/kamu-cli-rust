use chrono::prelude::*;
use indoc::indoc;
use std::convert::TryFrom;

use kamu::domain::*;
use kamu::infra::serde::yaml::*;

macro_rules! map(
    { $($key:expr => $value:expr),+ } => {
        {
            let mut m = ::std::collections::BTreeMap::new();
            $(
                m.insert($key, $value);
            )+
            m
        }
     };
);

macro_rules! yaml_map(
    { $($key:expr => $value:expr),+ } => {
        {
            let mut m = ::serde_yaml::Mapping::new();
            $(
                m.insert($key, $value);
            )+
            ::serde_yaml::Value::Mapping(m)
        }
     };
);

macro_rules! yaml_seq(
    () => (
        ::serde_yaml::Value::Sequence(vec!)
    );
    ($($x:expr),+ $(,)?) => (
        ::serde_yaml::Value::Sequence(vec![$($x),+])
    );
);

fn yaml_str(s: &str) -> serde_yaml::Value {
    serde_yaml::to_value(s).unwrap()
}

#[test]
fn de_dataset_snapshot_root() {
    let data = indoc!(
        "
        ---
        apiVersion: 1
        kind: DatasetSnapshot
        content:
          id: kamu.test
          source:
            kind: root
            fetch:
              kind: url
              url: ftp://kamu.dev/test.zip
              cache:
                kind: forever
            prepare:
            - kind: decompress
              format: zip
              subPath: data_*.csv
            read:
              kind: csv
              header: true
            preprocess:
              engine: sparkSQL
              query: >
                SELECT * FROM input
            merge:
              kind: snapshot
              primaryKey:
              - id
          vocab:
            eventTimeColumn: date"
    );

    let actual: Manifest<DatasetSnapshot> = serde_yaml::from_str(data).unwrap();

    let expected = Manifest {
        api_version: 1,
        kind: "DatasetSnapshot".to_owned(),
        content: DatasetSnapshot {
            id: DatasetIDBuf::try_from("kamu.test").unwrap(),
            source: DatasetSource::Root(DatasetSourceRoot {
                fetch: FetchStep::Url(FetchStepUrl {
                    url: "ftp://kamu.dev/test.zip".to_owned(),
                    event_time: None,
                    cache: Some(SourceCaching::Forever),
                }),
                prepare: Some(vec![PrepStep::Decompress(PrepStepDecompress {
                    format: CompressionFormat::Zip,
                    sub_path: Some("data_*.csv".to_owned()),
                })]),
                read: ReadStep::Csv(ReadStepCsv {
                    schema: None,
                    separator: None,
                    encoding: None,
                    quote: None,
                    escape: None,
                    comment: None,
                    header: Some(true),
                    enforce_schema: None,
                    infer_schema: None,
                    ignore_leading_white_space: None,
                    ignore_trailing_white_space: None,
                    null_value: None,
                    empty_value: None,
                    nan_value: None,
                    positive_inf: None,
                    negative_inf: None,
                    date_format: None,
                    timestamp_format: None,
                    multi_line: None,
                }),
                preprocess: Some(Transform {
                    engine: "sparkSQL".to_owned(),
                    additional_properties: map! {
                        "query".to_owned() => yaml_str("SELECT * FROM input\n")
                    },
                }),
                merge: MergeStrategy::Snapshot(MergeStrategySnapshot {
                    primary_key: vec!["id".to_owned()],
                    compare_columns: None,
                    observation_column: None,
                    obsv_added: None,
                    obsv_changed: None,
                    obsv_removed: None,
                }),
            }),
            vocab: Some(DatasetVocabulary {
                system_time_column: None,
                event_time_column: Some("date".to_owned()),
            }),
        },
    };

    assert_eq!(expected, actual);
}

#[test]
fn de_dataset_snapshot_derivative() {
    let data = indoc!(
        "
        ---
        apiVersion: 1
        kind: DatasetSnapshot
        content:
          id: com.naturalearthdata.admin0
          source:
            kind: derivative
            inputs:
            - com.naturalearthdata.10m.admin0
            - com.naturalearthdata.50m.admin0
            transform:
              engine: sparkSQL
              query: SOME_SQL"
    );

    let actual: Manifest<DatasetSnapshot> = serde_yaml::from_str(data).unwrap();

    let expected = Manifest {
        api_version: 1,
        kind: "DatasetSnapshot".to_owned(),
        content: DatasetSnapshot {
            id: DatasetIDBuf::try_from("com.naturalearthdata.admin0").unwrap(),
            source: DatasetSource::Derivative(DatasetSourceDerivative {
                inputs: vec![
                    DatasetIDBuf::try_from("com.naturalearthdata.10m.admin0").unwrap(),
                    DatasetIDBuf::try_from("com.naturalearthdata.50m.admin0").unwrap(),
                ],
                transform: Transform {
                    engine: "sparkSQL".to_owned(),
                    additional_properties: map! {
                        "query".to_owned() => yaml_str("SOME_SQL")
                    },
                },
            }),
            vocab: None,
        },
    };

    assert_eq!(expected, actual);
}

#[test]
fn de_metadata_block() {
    let data = indoc!(
        "
        ---
        apiVersion: 1
        kind: MetadataBlock
        content:
          blockHash: ddeeaaddbbeeff
          prevBlockHash: ffeebbddaaeedd
          systemTime: '2020-01-01T12:00:00.000Z'
          source:
            kind: derivative
            inputs:
            - input1
            - input2
            transform:
              engine: sparkSQL
              query: >
                SELECT * FROM input1 UNION ALL SELECT * FROM input2
          outputSlice:
            hash: ffaabb
            interval: '[2020-01-01T12:00:00.000Z, 2020-01-01T12:00:00.000Z]'
            numRecords: 10
          outputWatermark: '2020-01-01T12:00:00.000Z'
          inputSlices:
          - hash: aa
            interval: '(-inf, 2020-01-01T12:00:00.000Z]'
            numRecords: 10
          - hash: zz
            interval: '()'
            numRecords: 0"
    );

    let actual: Manifest<MetadataBlock> = serde_yaml::from_str(data).unwrap();

    let expected = Manifest {
        api_version: 1,
        kind: "MetadataBlock".to_owned(),
        content: MetadataBlock {
            block_hash: "ddeeaaddbbeeff".to_owned(),
            prev_block_hash: "ffeebbddaaeedd".to_owned(),
            system_time: Utc.ymd(2020, 1, 1).and_hms(12, 0, 0),
            source: Some(DatasetSource::Derivative(DatasetSourceDerivative {
                inputs: vec![
                    DatasetIDBuf::try_from("input1").unwrap(),
                    DatasetIDBuf::try_from("input2").unwrap(),
                ],
                transform: Transform {
                    engine: "sparkSQL".to_owned(),
                    additional_properties: map! {
                        "query".to_owned() => yaml_str("SELECT * FROM input1 UNION ALL SELECT * FROM input2\n")
                    },
                },
            })),
            output_slice: Some(DataSlice {
                hash: "ffaabb".to_owned(),
                interval: TimeInterval::singleton(Utc.ymd(2020, 1, 1).and_hms(12, 0, 0)),
                num_records: 10,
            }),
            output_watermark: Some(Utc.ymd(2020, 1, 1).and_hms(12, 0, 0)),
            input_slices: Some(vec![
                DataSlice {
                    hash: "aa".to_owned(),
                    interval: TimeInterval::unbounded_closed_right(
                        Utc.ymd(2020, 1, 1).and_hms(12, 0, 0),
                    ),
                    num_records: 10,
                },
                DataSlice {
                    hash: "zz".to_owned(),
                    interval: TimeInterval::empty(),
                    num_records: 0,
                },
            ]),
        },
    };

    assert_eq!(expected, actual);
}

#[test]
fn serde_dataset_summary() {
    let data = indoc!(
        "
        ---
        apiVersion: 1
        kind: DatasetSummary
        content:
          id: foo.bar
          kind: root
          dependencies:
            - foo
            - bar
          lastPulled: '2020-01-01T12:00:00.000Z'
          dataSize: 1024
          numRecords: 100
          vocab: {}"
    );

    let actual: Manifest<DatasetSummary> = serde_yaml::from_str(data).unwrap();

    let expected = Manifest {
        api_version: 1,
        kind: "DatasetSummary".to_owned(),
        content: DatasetSummary {
            id: DatasetIDBuf::try_from("foo.bar").unwrap(),
            kind: DatasetKind::Root,
            dependencies: vec![
                DatasetIDBuf::try_from("foo").unwrap(),
                DatasetIDBuf::try_from("bar").unwrap(),
            ],
            last_pulled: Some(Utc.ymd(2020, 1, 1).and_hms(12, 0, 0)),
            data_size: 1024,
            num_records: 100,
            vocab: DatasetVocabulary::default(),
        },
    };

    assert_eq!(expected, actual);

    assert_eq!(
        serde_yaml::to_string(&actual).unwrap(),
        indoc!(
            "
            ---
            apiVersion: 1
            kind: DatasetSummary
            content:
              id: foo.bar
              kind: root
              dependencies:
                - foo
                - bar
              lastPulled: \"2020-01-01T12:00:00.000Z\"
              numRecords: 100
              dataSize: 1024
              vocab: {}"
        )
    );
}

#[test]
fn serde_fetch_step_files_glob() {
    let data = indoc!(
        "
        ---
        kind: filesGlob
        path: /opt/x/*.txt
        cache:
          kind: forever
        order: byName"
    );

    let actual: FetchStep = serde_yaml::from_str(data).unwrap();

    let expected = FetchStep::FilesGlob(FetchStepFilesGlob {
        path: "/opt/x/*.txt".to_owned(),
        event_time: None,
        cache: Some(SourceCaching::Forever),
        order: Some(SourceOrdering::ByName),
    });

    assert_eq!(expected, actual);

    assert_eq!(
        serde_yaml::to_string(&actual).unwrap(),
        indoc!(
            "
            ---
            kind: filesGlob
            path: /opt/x/*.txt
            cache:
              kind: forever
            order: byName"
        )
    );
}

#[test]
fn serde_transform() {
    let data = indoc!(
        "
        ---
        engine: flink
        temporalTables:
        - id: foo
          primaryKey:
          - id
        queries:
        - alias: bar
          query: >
            SELECT * FROM foo"
    );

    let actual: Transform = serde_yaml::from_str(data).unwrap();

    let expected = Transform {
        engine: "flink".to_owned(),
        additional_properties: map! {
            "temporalTables".to_owned() => yaml_seq![
                yaml_map! {
                    yaml_str("id") => yaml_str("foo"),
                    yaml_str("primaryKey") => yaml_seq![yaml_str("id")]
                }
            ],
            "queries".to_owned() => yaml_seq![
                yaml_map! {
                    yaml_str("alias") => yaml_str("bar"),
                    yaml_str("query") => yaml_str("SELECT * FROM foo")
                }
            ]
        },
    };

    assert_eq!(expected, actual);

    assert_eq!(
        serde_yaml::to_string(&actual).unwrap(),
        indoc!(
            "
            ---
            engine: flink
            queries:
              - alias: bar
                query: SELECT * FROM foo
            temporalTables:
              - id: foo
                primaryKey:
                  - id"
        )
    );
}
