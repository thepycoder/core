use arrow::array::{
    Array, ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, StringArray, UInt32Array,
    UInt64Array,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

pub fn read_string_column(batch: &RecordBatch, name: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let idx = batch.schema().index_of(name)?;
    let col = batch.column(idx);
    macro_rules! stringify {
        ($array_type:ty) => {
            if let Some(arr) = col.as_any().downcast_ref::<$array_type>() {
                return Ok((0..arr.len())
                    .map(|i| {
                        if arr.is_null(i) {
                            String::new()
                        } else {
                            arr.value(i).to_string()
                        }
                    })
                    .collect());
            }
        };
    }
    stringify!(StringArray);
    stringify!(UInt32Array);
    stringify!(UInt64Array);
    stringify!(Int32Array);
    stringify!(Int64Array);
    stringify!(BooleanArray);
    stringify!(Float64Array);
    Err(format!("column {name} has unsupported type {}", col.data_type()).into())
}

pub fn read_u32_column(batch: &RecordBatch, name: &str) -> Result<Vec<u32>, Box<dyn Error>> {
    let idx = batch.schema().index_of(name)?;
    let col = batch.column(idx);
    let arr = col
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| format!("column {name} is not UInt32"))?;
    Ok((0..arr.len())
        .map(|i| if arr.is_null(i) { 0 } else { arr.value(i) })
        .collect())
}

pub fn read_bool_column(batch: &RecordBatch, name: &str) -> Result<Vec<bool>, Box<dyn Error>> {
    let idx = batch.schema().index_of(name)?;
    let col = batch.column(idx);
    let arr = col
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or_else(|| format!("column {name} is not Boolean"))?;
    Ok((0..arr.len())
        .map(|i| !arr.is_null(i) && arr.value(i))
        .collect())
}

pub fn read_f64_column(batch: &RecordBatch, name: &str) -> Result<Vec<f64>, Box<dyn Error>> {
    let idx = batch.schema().index_of(name)?;
    let col = batch.column(idx);
    let arr = col
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| format!("column {name} is not Float64"))?;
    Ok((0..arr.len())
        .map(|i| if arr.is_null(i) { 0.0 } else { arr.value(i) })
        .collect())
}

pub fn read_optional_string_column(
    batch: &RecordBatch,
    name: &str,
) -> Result<Vec<Option<String>>, Box<dyn Error>> {
    let idx = batch.schema().index_of(name)?;
    let col = batch.column(idx);
    let arr = col
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| format!("column {name} is not Utf8"))?;
    let mut out = Vec::with_capacity(arr.len());
    for i in 0..arr.len() {
        if arr.is_null(i) {
            out.push(None);
        } else {
            out.push(Some(arr.value(i).to_string()));
        }
    }
    Ok(out)
}

pub fn read_all_rows(path: &Path) -> Result<Vec<RecordBatch>, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch?);
    }
    Ok(batches)
}

pub fn column_values(path: &Path, column: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut values = Vec::new();
    for batch in read_all_rows(path)? {
        values.extend(read_string_column(&batch, column)?);
    }
    Ok(values)
}

pub fn write_parquet(
    path: &Path,
    schema: Schema,
    columns: Vec<ArrayRef>,
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let schema = Arc::new(schema);
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn utf8_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Utf8, nullable)
}
