use crate::{
    error::{c4error_init, Error, Result},
    ffi::{
        c4query_new2, c4query_release, c4query_run, c4query_setParameters, c4queryenum_next,
        c4queryenum_release, kC4DefaultQueryOptions, C4Query, C4QueryEnumerator, C4String,
        FLArrayIterator_GetCount, FLArrayIterator_GetValueAt,
    },
    value::{FromValueRef, ValueRef},
    Database, QueryLanguage,
};
use couchbase_lite_core_sys::FLStringResult;
use fallible_streaming_iterator::FallibleStreamingIterator;
use serde::Serialize;
use std::ptr::NonNull;

pub struct Query<'db> {
    _db: &'db Database,
    inner: NonNull<C4Query>,
}

impl Drop for Query<'_> {
    fn drop(&mut self) {
        unsafe { c4query_release(self.inner.as_ptr()) };
    }
}

impl Query<'_> {
    pub(crate) fn new<'a, 'b>(
        db: &'a Database,
        query_lang: QueryLanguage,
        query: &'b str,
    ) -> Result<Query<'a>> {
        let mut c4err = c4error_init();
        let mut out_error_pos = -1;
        let query = unsafe {
            c4query_new2(
                db.inner.0.as_ptr(),
                query_lang,
                query.into(),
                &mut out_error_pos,
                &mut c4err,
            )
        };

        NonNull::new(query)
            .map(|inner| Query { _db: db, inner })
            .ok_or_else(|| c4err.into())
    }

    /// convinient function to call with macros `serde_fleece::fleece`
    /// as parameter
    pub fn set_parameters_fleece(
        &self,
        parameters: std::result::Result<FLStringResult, serde_fleece::Error>,
    ) -> Result<()> {
        let params = parameters?;
        unsafe {
            c4query_setParameters(self.inner.as_ptr(), params.as_fl_slice());
        }
        Ok(())
    }

    pub fn set_parameters<T>(&self, parameters: &T) -> Result<()>
    where
        T: Serialize,
    {
        let param_string = serde_fleece::to_fl_slice_result(parameters)?;
        unsafe {
            c4query_setParameters(self.inner.as_ptr(), param_string.as_fl_slice());
        }
        Ok(())
    }

    pub fn run(&self) -> Result<Enumerator> {
        let mut c4err = c4error_init();
        let it = unsafe {
            c4query_run(
                self.inner.as_ptr(),
                &kC4DefaultQueryOptions,
                C4String::default(),
                &mut c4err,
            )
        };

        NonNull::new(it)
            .map(|inner| Enumerator {
                _query: self,
                reach_end: false,
                inner,
            })
            .ok_or_else(|| c4err.into())
    }
}

pub struct Enumerator<'query> {
    _query: &'query Query<'query>,
    reach_end: bool,
    inner: NonNull<C4QueryEnumerator>,
}

impl Drop for Enumerator<'_> {
    fn drop(&mut self) {
        unsafe { c4queryenum_release(self.inner.as_ptr()) };
    }
}

impl<'en> FallibleStreamingIterator for Enumerator<'en> {
    type Error = crate::error::Error;
    type Item = Enumerator<'en>;

    fn advance(&mut self) -> Result<()> {
        if self.reach_end {
            return Ok(());
        }
        let mut c4err = c4error_init();
        if unsafe { c4queryenum_next(self.inner.as_ptr(), &mut c4err) } {
            Ok(())
        } else if c4err.code == 0 {
            self.reach_end = true;
            Ok(())
        } else {
            Err(c4err.into())
        }
    }

    fn get(&self) -> Option<&Enumerator<'en>> {
        if !self.reach_end {
            Some(self)
        } else {
            None
        }
    }
}

impl<'a> Enumerator<'a> {
    pub fn get_raw_checked(&self, i: u32) -> Result<ValueRef<'a>> {
        let n = unsafe { FLArrayIterator_GetCount(&self.inner.as_ref().columns) };
        if i >= n {
            return Err(Error::LogicError(format!(
                "Enumerator::get_raw_checked: Index out of bounds {} / {}",
                i, n
            )));
        }

        let val: ValueRef =
            unsafe { FLArrayIterator_GetValueAt(&self.inner.as_ref().columns, i) }.into();
        Ok(val)
    }

    pub fn get_checked<T>(&self, i: u32) -> Result<T>
    where
        T: FromValueRef<'a>,
    {
        let value_ref = self.get_raw_checked(i)?;
        FromValueRef::column_result(value_ref)
    }
}
