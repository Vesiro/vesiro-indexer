# vesiro-indexer

Creating Elasticsearch indices is a key part of developing [VesiroSearch](https://vesiro.com), our
plugin for optimizing query latency. We need those indices to be reliable, to be any size we ask
for, and to survive a failure partway through, so we wrote vesiro-indexer. It sources its documents
from [Common Crawl](https://commoncrawl.org/), and gives you detailed control over what ends up in the
Elasticsearch indices.

## Installing

It is a Rust workspace, so the usual applies:

```sh
cargo install --path indexer
```

Or build it in place with `cargo build --release` and run it out of `target/release/`.

## Usage

Everything happens through two commands. `index` talks to a node; `records` talks to the indexer's
own bookkeeping.

For a full list of options use:
```sh
vesiro-indexer help
```

### Creating an index

An index is created on the Elasticsearch cluster with the following command:

```sh
vesiro-indexer index \
  --node-url http://node.example:9200/ \
  create \
  --collection cc-wet2024-shuffled \
  --mapping cc-wet \
  --codec best_compression \
  --index-name myindex
```

### Populating an index

```sh
vesiro-indexer index \
  --node-url http://node.example:9200/ \
  populate \
  --index-name myindex \
  --doc-delta 1000000 \
  --worker-amount 4 \
  --refresh-when-done
```

This command will populate the index `myindex` with 1,000,000 documents. It will use 4 concurrent
clients to send documents to the Elasticsearch cluster.

Increasing the amount of workers can increase the speed at which documents are added to the index,
but each worker potentially allocates a substantial amount of memory. Adjust the worker amount
according to your system's available resources.

### Populating an index again

Want to add more documents to an index after you have populated it? Just run the command again.
vesiro-indexer keeps records of which documents it has already successfully added to an
Elasticsearch index and will continue from where it left off.

### Mapping

A mapping describes the structure of the fields of the documents in an index. For more information,
consult [the official Elasticsearch documentation][es-mapping]. The mapping must be chosen at the
creation of an index. vesiro-indexer currently provides three predefined mappings:
- raw
- cc-wet
- cc-warc

[es-mapping]: https://www.elastic.co/docs/manage-data/data-store/mapping

### Collections

A collection is a dataset of raw Common Crawl files, each containing a few thousand documents.

| Collection | Mapping | What is in it |
| --- | --- | --- |
| `cc-warc2024` | `cc-warc` | Documents containing page HTML and a full set of metadata |
| `cc-warc2024-shuffled` | `cc-warc` | The same, visited in shuffled order |
| `cc-wet2024` | `cc-wet` | The extracted plain text of each page, one document per page |
| `cc-wet2024-shuffled` | `cc-wet` | The same, visited in shuffled order |

The `raw` mapping accepts any of the four collections.

The collections that are not shuffled will provide the Common Crawl files in chronological order.
Given the magnitude of the Common Crawl dataset, this means that the first million documents will
all be from the first eight minutes of February 20, 2024. The shuffled variants shuffle the files
so the documents will be sourced from all around the year.

### Object Source

`--object-source` chooses which of the two sources the Common Crawl files (or objects as AWS
calls files on S3) are read from. It defaults to `s3`.

| Source | What it reads | Needs |
| --- | --- | --- |
| `s3` | Common Crawl's `commoncrawl` bucket, read directly | AWS credentials in the environment |
| `server` | A vesiro-indexer server which serves the files it has already downloaded | `--server-url` |

Reading from `s3` needs an AWS account, because the bucket refuses unsigned requests. Any account
works and the data is free. Grant your identity `s3:GetObject` on `arn:aws:s3:::commoncrawl/*`,
then supply credentials the way the AWS SDKs expect. Either in `~/.aws/credentials` or in
[the .env file](#aws-credentials).

S3 limits how many requests a single user can make, and sustained downloading starts coming back
as HTTP 503. On an index of any real size (1TiB or more) this is more or less inevitable.

The `server` source avoids the limit, because the files it serves have already been fetched and
sit on its disk. We cannot open our own server to the public, since we are not allowed to
redistribute the Common Crawl dataset. We may release the server software later, so you can run
your own and download your own data.

### Records

vesiro-indexer uses an internal database to keep track of the documents it has successfully
populated an Elasticsearch cluster with. This way it is possible to add more documents to an index
at any point.

Records are managed by the subcommand `records`. The records API is not something the basic usage of
vesiro-indexer really touches. For the full API consult the help pages:

```sh
vesiro-indexer records help
```

## The security plugin and TLS

A node with the security plugin turned on will not answer any of this without credentials. Pass
them with `--user`, as one `username:password` argument, on the `index` command or on
`records delete`:

```sh
vesiro-indexer index \
  --node-url https://node.example:9200/ \
  --user username:password \
  populate --index-name myindex --doc-delta 1000000
```

> [!CAUTION]
> Passing `--user` currently builds a client that accepts any certificate.

## Configuration with `.env`

Every option listed below can be set in a `.env` file instead of being typed out on every run. The
file is read from the working directory, or the nearest parent holding one, before the options are
parsed. A value given on the command line always wins over the file.

| Variable | Option |
| --- | --- |
| `VESIRO_INDEXER_RECORDS_PATH` | `--records-path` |
| `VESIRO_INDEXER_NODE_URL` | `index --node-url` |
| `VESIRO_INDEXER_SERVER_URL` | `index populate --server-url` |
| `VESIRO_INDEXER_USER` | `index --user`, `records delete --user` |
| `VESIRO_INDEXER_COLLECTION` | `index create --collection` |
| `VESIRO_INDEXER_MAPPING` | `index create --mapping` |
| `VESIRO_INDEXER_CODEC` | `index create --codec` |
| `VESIRO_INDEXER_NUMBER_OF_SHARDS` | `index create --number-of-shards` |
| `VESIRO_INDEXER_NUMBER_OF_REPLICAS` | `index create --number-of-replicas` |
| `VESIRO_INDEXER_ENABLE_SOURCE` | `index create --enable-source` |
| `VESIRO_INDEXER_INDEX_NAME` | `index create --index-name`, `index populate --index-name` |
| `VESIRO_INDEXER_WORKER_AMOUNT` | `index populate --worker-amount` |
| `VESIRO_INDEXER_OBJECT_SOURCE` | `index populate --object-source` |

`VESIRO_INDEXER_ENABLE_SOURCE` takes a value rather than being set by its presence: an empty
string, `0`, `false`, `no` or `off` turn it off, anything else turns it on.

`RUST_LOG` is read from the file as well.

### AWS Credentials

If you are using S3 as the object source you can add your AWS credentials to the `.env` file
under `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.

## Logging

Logging goes to stderr and is controlled by `RUST_LOG`, defaulting to `vesiro_indexer=info`.
