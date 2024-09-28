/// the buffer's current content
pub struct SubjectBufferSnapshot<'a> {
    pub buffer: &'a[u8],
    pub match_offset: usize,
}

pub struct SubjectBuffer {
    buffer: Box<[u8]>,

    /// buffer capacity will be smaller than min_capacity before first read,
    /// but will be greater or equal after first read
    min_capacity: usize,

    /// buffer capacity will be doubled unless it would exceed this value
    max_capacity: usize,

    /// a property of the pattern
    max_lookbehind: usize,

    /// the number of bytes in the buffer
    len: usize,

    /// the current position of pattern matching
    match_offset: usize,

    /// indicates the position of the buffer's beginning inside of the source.  
    /// it may start as a negative value, as the start is padded with zeroed lookbehind bytes
    source_offset: i128,
}

impl SubjectBuffer {
    pub fn new(min_capacity: usize, max_capacity: usize, max_lookbehind: usize) -> Result<Self, Box<dyn std::error::Error>> {
        if min_capacity == 0 {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "the minimum capacity must be non-zero")));
        }
        if min_capacity <= max_lookbehind {
            let err_msg = format!("the minimum capacity ({}) must be increased to surpass the maximum lookbehind ({})", min_capacity, max_capacity);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, err_msg)));
        }
        // no special handling or assertions is required for max_capacity

        Ok(Self {
            buffer: vec![0; max_lookbehind].into_boxed_slice(),
            min_capacity,
            max_capacity,
            max_lookbehind,
            len: max_lookbehind,
            match_offset: max_lookbehind,
            source_offset: -(max_lookbehind as i128),
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// gives the max_lookbehind, as provided in the ctor.
    /// useful as the initial arg to read
    pub fn max_lookbehind(&self) -> usize {
        self.max_lookbehind
    }

    pub fn min_capacity(&self) -> usize {
        self.min_capacity
    }

    // max_capacity can be < min_capacity - will simply err when trying to get more space
    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }
 
    /// read from the input_source into the buffer. new match offset indicates
    /// the point where matching has stopped.
    ///  - on first read, this must be equal to the max lookbehind (zero for no lookbehind)
    ///  - otherwise, point to beginning of an incomplete match (not including lookbehind)
    ///  - otherwise, on no matches remaining, point to the end of the buffer (get_size())
    ///
    /// true iff the input is complete (and 0 bytes were added to the buffer)
    /// 
    /// 1. read
    /// 2. get_snapshot
    /// 3. <do pattern matching>
    /// 4. verify_match
    /// 5. get_absolute_offset
    pub fn read<R: std::io::Read>(
        &mut self,
        new_match_offset: usize,
        input_source: &mut R,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        debug_assert!(new_match_offset >= self.match_offset);
        self.match_offset = new_match_offset;
        debug_assert!(self.match_offset <= self.len);

        if self.match_offset <= self.max_lookbehind {
            // atypical case. no bytes can safely be discarded from the buffer. this
            // is handled by expanding the size of the buffer
            let next_cap = if self.buffer.len() < self.min_capacity {
                // this always occurs on first read.

                // buffer len was originally set to max_lookbehind.
                // the min_capacity is always greater than the max lookbehind.
                // this is checked in the ctor
                self.min_capacity
            } else {
                let next_cap = self.buffer.len() * 2;
                if next_cap > self.max_capacity {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "match length would exceed maximum buffer capacity",
                    )));
                }
                next_cap
            };

            let mut new_buffer = vec![0; next_cap].into_boxed_slice();
            (&mut new_buffer[0..self.len]).copy_from_slice(&self.buffer[0..self.len]);
            self.buffer = new_buffer
        } else {
            // typical case. see readme docstring for details
            let num_bytes_discarded = self.match_offset - self.max_lookbehind;
            debug_assert!(num_bytes_discarded > 0); // guarded against, above
            self.buffer.copy_within(num_bytes_discarded..self.len, 0);
            self.len -= num_bytes_discarded;
            self.match_offset -= num_bytes_discarded;
            self.source_offset += num_bytes_discarded as i128;
        }

        // more space was made above. fill it
        let len = self.buffer.len();
        let mut read_dst = &mut self.buffer[self.len..len];
        match input_source.read(&mut read_dst) {
            Ok(read_ret) => {
                self.len += read_ret;
                return Ok(read_ret==0);
            },
            Err(e) => return Err(Box::new(e)),
        }
    }

    /// returns buffer content at and following the match offset
    pub fn bytes_to_process<'a>(&'a self) -> &'a [u8] {
        debug_assert!(self.match_offset >= self.max_lookbehind);
        debug_assert!(self.match_offset <= self.len);
        &self.buffer[self.match_offset..self.len]
    }
    
    /// returns the buffer, and the position of matching within it. under all
    /// circumstances, the match offset will be proceeded by enough bytes to
    /// satisfy the lookbehind; at the beginning of the source, the beginning is
    /// padded with null bytes to make this true.
    pub fn get_snapshot<'a>(&'a self) -> SubjectBufferSnapshot<'a> {
        debug_assert!(self.match_offset >= self.max_lookbehind);
        debug_assert!(self.match_offset <= self.len);
        SubjectBufferSnapshot {
            buffer: &self.buffer[..self.len],
            match_offset: self.match_offset,
        }
    }

    /// the beginning of the source is padded with null bytes to always have a
    /// sufficient lookbehind length. this function checks that a match's
    /// lookbehind does not include this fake padding
    pub fn verify_match(&self, match_begin_with_lookbehind: usize) -> bool {
        if self.source_offset >= 0 {
            return true
        }
        return (match_begin_with_lookbehind as i128) >= -self.source_offset
    }

    /// a match offset is relative to the beginning of the matching buffer.
    /// this translates a match offset to an offset within the source.
    pub fn get_absolute_offset(&self, match_offset: usize) -> i128 {
        match_offset as i128 + self.source_offset
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn empty_min_cap_not_allowed() {
        let buffer = SubjectBuffer::new(0, 0, 0);
        match buffer {
            Ok(_) => assert!(false),
            Err(_) => assert!(true),
        }
    }

    #[test]
    fn min_capacity_too_small() {
        let buffer = SubjectBuffer::new(1, 0, 2);
        match buffer {
            Ok(_) => assert!(false),
            Err(_) => assert!(true),
        }
    }

    #[test]
    fn min_capacity_equal_max_lookbehind() {
        let buffer = SubjectBuffer::new(1, 0, 1);
        match buffer {
            Ok(_) => assert!(false),
            Err(_) => assert!(true),
        }
    }

    #[test]
    fn simple_case() {
        let mut buffer = SubjectBuffer::new(20, 0, 0).unwrap();
        let data: &[u8] = b"Hello, world!";
        let mut reader = Cursor::new(data);

        let ret = buffer.read(buffer.max_lookbehind(), &mut reader).unwrap();
        assert!(!ret); // not complete because bytes were read

        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == data); 
        assert!(match_buffer.match_offset == 0);

        let ret = buffer.read(buffer.len, &mut reader).unwrap();
        assert!(buffer.source_offset as usize == data.len());
        assert!(ret); // input complete
    }

    #[test]
    fn simple_chunks() {
        let mut buffer = SubjectBuffer::new(1, 0, 0).unwrap();
        let data: &[u8] = b"Hello, world!";
        let mut reader = Cursor::new(data);

        assert_eq!(buffer.bytes_to_process(), &[]);
        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == &[]); 
        assert!(match_buffer.match_offset == 0);
        assert!(buffer.source_offset == 0);

        let ret = buffer.read(buffer.max_lookbehind(), &mut reader).unwrap();
        assert!(!ret);

        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == &data[0..1]); 
        assert!(match_buffer.match_offset == 0);
        assert!(buffer.source_offset == 0);
        assert_eq!(buffer.get_absolute_offset(0), 0);

        let ret = buffer.read(buffer.len, &mut reader).unwrap();
        assert!(!ret);

        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == &data[1..2]); 
        assert!(match_buffer.match_offset == 0);
        assert!(buffer.source_offset == 1);
        assert_eq!(buffer.get_absolute_offset(1), 2);

        let ret = buffer.read(buffer.len, &mut reader).unwrap();
        assert!(!ret);

        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == &data[2..3]); 
        assert!(match_buffer.match_offset == 0);
        assert!(buffer.source_offset == 2);
    }

    #[test]
    fn simple_chunks_with_lookbehind() {
        let mut buffer = SubjectBuffer::new(2, 0, 1).unwrap();
        let data: &[u8] = b"Hello, world!";
        let mut reader = Cursor::new(data);

        assert_eq!(buffer.bytes_to_process(), &[]);
        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == &[b'\0']); 
        assert!(match_buffer.match_offset == 1);
        assert!(buffer.source_offset == -1);

        let ret = buffer.read(buffer.max_lookbehind(), &mut reader).unwrap();
        assert!(!ret);
        let match_buffer = buffer.get_snapshot();
        assert_eq!(buffer.bytes_to_process(), &[b'H']);
        assert!(match_buffer.buffer == &[b'\0', b'H']); 
        assert!(match_buffer.match_offset == 1);
        assert!(buffer.source_offset == -1);

        assert_eq!(false, buffer.verify_match(0));
        assert_eq!(true, buffer.verify_match(1));

        let ret = buffer.read(buffer.len, &mut reader).unwrap();
        assert!(!ret);

        let match_buffer = buffer.get_snapshot();
        assert!(match_buffer.buffer == &[b'H', b'e']); 
        assert!(match_buffer.match_offset == 1);
        assert!(buffer.source_offset == 0);

        assert_eq!(true, buffer.verify_match(0));
        assert_eq!(true, buffer.verify_match(1));
    }

    #[test]
    fn test_realloc() {
        let mut buffer = SubjectBuffer::new(2, 4, 0).unwrap();
        let data: &[u8] = b"Hello, world!";
        let mut reader = Cursor::new(data);

        let ret = buffer.read(buffer.max_lookbehind(), &mut reader).unwrap();
        assert!(!ret);
        assert_eq!(buffer.len, 2); // set to intial size

        // let's say the match offset hasn't been moved forward (still at the beginning)
        let ret = buffer.read(buffer.max_lookbehind(), &mut reader).unwrap();
        assert!(!ret);
        let b = buffer.get_snapshot();
        assert_eq!(b.buffer, &data[0..4]);
        assert_eq!(b.match_offset, 0);

        // max cap would be surpassed
        let ret = buffer.read(buffer.max_lookbehind(), &mut reader);
        match ret {
            Ok(_) => assert!(false),
            Err(_) => assert!(true),
        }
    }
}
