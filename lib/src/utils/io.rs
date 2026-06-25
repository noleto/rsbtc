use std::{
    fs::File,
    io::{Error as IoError, ErrorKind as IoErrorKind, Read, Result as IoResult, Write},
    path::Path,
};

pub trait AutoSaveable {}

pub trait Saveable
where
    Self: Sized,
{
    fn load<I: Read>(reader: I) -> IoResult<Self>;
    fn save<O: Write>(&self, writer: O) -> IoResult<()>;

    fn save_to_file<P: AsRef<Path>>(&self, path: P) -> IoResult<()> {
        let file = File::create(&path)?;
        self.save(file)
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> IoResult<Self> {
        let file = File::open(&path)?;
        Self::load(file)
    }
}

impl<T> Saveable for T
where
    T: Sized + serde::Serialize + serde::de::DeserializeOwned + AutoSaveable,
{
    fn load<I: Read>(reader: I) -> IoResult<Self> {
        ciborium::de::from_reader(reader).map_err(|_| {
            IoError::new(
                IoErrorKind::InvalidData,
                format!("Failed to deserialize {}", std::any::type_name::<Self>()),
            )
        })
    }

    fn save<O: Write>(&self, writer: O) -> IoResult<()> {
        ciborium::ser::into_writer(self, writer).map_err(|_| {
            IoError::new(
                IoErrorKind::InvalidData,
                format!("Failed to serialize {}", std::any::type_name::<Self>()),
            )
        })?;
        Ok(())
    }
}
