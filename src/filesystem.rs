//! Simple filesystem with snapshot support
//!
//! Features:
//! - Basic file operations (create, read, write, delete)
//! - Directory structure
//! - Copy-on-write snapshots for rollback
//! - Block device abstraction for storage backends

use crate::frame_allocator::{PhysAddr, Frame, allocate_frame, deallocate_frame};
use core::mem;

/// Block device trait for storage backends
pub trait BlockDevice {
    /// Read a block from the device
    fn read_block(&self, block_num: BlockNum, buffer: &mut [u8]) -> Result<(), &'static str>;
    
    /// Write a block to the device
    fn write_block(&self, block_num: BlockNum, buffer: &[u8]) -> Result<(), &'static str>;
    
    /// Get the total number of blocks
    fn total_blocks(&self) -> BlockNum;
    
    /// Get the block size
    fn block_size(&self) -> usize;
}

/// In-memory block device for testing (current implementation)
struct MemoryBlockDevice {
    total_blocks: BlockNum,
    blocks: *mut [u8; BLOCK_SIZE],
}

impl MemoryBlockDevice {
    fn new(total_blocks: BlockNum) -> Self {
        let layout = core::alloc::Layout::array::<[u8; BLOCK_SIZE]>(total_blocks as usize).unwrap();
        let blocks = unsafe { core::alloc::alloc(layout) as *mut [u8; BLOCK_SIZE] };
        
        // Initialize to zero
        unsafe {
            core::ptr::write_bytes(blocks, 0, total_blocks as usize);
        }
        
        MemoryBlockDevice {
            total_blocks,
            blocks,
        }
    }
}

impl BlockDevice for MemoryBlockDevice {
    fn read_block(&self, block_num: BlockNum, buffer: &mut [u8]) -> Result<(), &'static str> {
        if block_num >= self.total_blocks || buffer.len() != BLOCK_SIZE {
            return Err("Invalid block number or buffer size");
        }
        
        unsafe {
            let block = &*self.blocks.add(block_num as usize);
            buffer.copy_from_slice(block);
        }
        
        Ok(())
    }
    
    fn write_block(&self, block_num: BlockNum, buffer: &[u8]) -> Result<(), &'static str> {
        if block_num >= self.total_blocks || buffer.len() != BLOCK_SIZE {
            return Err("Invalid block number or buffer size");
        }
        
        unsafe {
            let block = &mut *self.blocks.add(block_num as usize);
            block.copy_from_slice(buffer);
        }
        
        Ok(())
    }
    
    fn total_blocks(&self) -> BlockNum {
        self.total_blocks
    }
    
    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }
}

/// AHCI block device implementation
struct AhciBlockDevice;

impl AhciBlockDevice {
    fn new() -> Option<Self> {
        // Check if AHCI controller is available
        if crate::ahci::get_controller().is_some() {
            Some(AhciBlockDevice)
        } else {
            None
        }
    }
}

impl BlockDevice for AhciBlockDevice {
    fn read_block(&self, block_num: BlockNum, buffer: &mut [u8]) -> Result<(), &'static str> {
        if let Some(controller) = crate::ahci::get_controller() {
            // Convert block number to LBA (assuming 512-byte sectors)
            let lba = block_num as u64 * (BLOCK_SIZE as u64 / 512);
            let sectors_per_block = (BLOCK_SIZE / 512) as u8;
            
            // For now, read from first available port
            if let Some(port) = controller.get_port(0) {
                if port.read_sectors(lba, sectors_per_block, buffer).is_ok() {
                    return Ok(());
                }
            }
        }
        Err("AHCI read failed")
    }
    
    fn write_block(&self, block_num: BlockNum, buffer: &[u8]) -> Result<(), &'static str> {
        if let Some(controller) = crate::ahci::get_controller() {
            // Convert block number to LBA (assuming 512-byte sectors)
            let lba = block_num as u64 * (BLOCK_SIZE as u64 / 512);
            let sectors_per_block = (BLOCK_SIZE / 512) as u8;
            
            // For now, write to first available port
            if let Some(port) = controller.get_port(0) {
                if port.write_sectors(lba, sectors_per_block, buffer).is_ok() {
                    return Ok(());
                }
            }
        }
        Err("AHCI write failed")
    }
    
    fn total_blocks(&self) -> BlockNum {
        // For now, assume a reasonable size (would be detected from disk)
        1024 * 1024 // 4GB worth of 4KB blocks
    }
    
    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }
}

/// Block size (4KB, matches frame size)
pub const BLOCK_SIZE: usize = 4096;

/// Maximum filename length
pub const MAX_FILENAME_LEN: usize = 255;

/// Maximum path length
pub const MAX_PATH_LEN: usize = 4096;

/// Inode number type
pub type InodeNum = u32;

/// Block number type
pub type BlockNum = u32;

/// File descriptor type
pub type FileDescriptor = u32;

/// Open file flags
#[derive(Debug, Clone, Copy)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
}

impl OpenFlags {
    pub fn from_bits(bits: u32) -> Option<Self> {
        Some(OpenFlags {
            read: bits & 0x1 != 0,
            write: bits & 0x2 != 0,
            create: bits & 0x4 != 0,
            truncate: bits & 0x8 != 0,
        })
    }
}

/// File type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    Regular,
    Directory,
}

/// File permissions (simplified)
#[derive(Debug, Clone, Copy)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// Inode structure
#[derive(Debug, Clone)]
pub struct Inode {
    pub inum: InodeNum,
    pub file_type: FileType,
    pub size: usize,
    pub permissions: Permissions,
    pub uid: u32,
    pub gid: u32,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub blocks: [BlockNum; 12], // Direct blocks
    pub indirect_block: BlockNum, // Single indirect
    pub double_indirect_block: BlockNum, // Double indirect
    pub triple_indirect_block: BlockNum, // Triple indirect
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: [u8; MAX_FILENAME_LEN],
    pub name_len: u8,
    pub inum: InodeNum,
}

/// Superblock
#[derive(Debug, Clone)]
pub struct Superblock {
    pub magic: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub free_blocks: u32,
    pub total_inodes: u32,
    pub free_inodes: u32,
    pub root_inode: InodeNum,
    pub snapshot_root: InodeNum,
}

/// Filesystem instance
pub struct Filesystem {
    block_device: &'static dyn BlockDevice,
    superblock: Superblock,
    inode_bitmap: &'static mut [u64],
    block_bitmap: &'static mut [u64],
    inodes: &'static mut [Inode],
    current_snapshot: InodeNum,
    next_fd: FileDescriptor,
    open_files: [Option<OpenFile>; 256], // Simple fixed-size table
}

/// Open file entry
#[derive(Clone, Copy)]
struct OpenFile {
    inum: InodeNum,
    position: usize,
    flags: OpenFlags,
}

impl Filesystem {
    /// Initialize filesystem on disk
    pub fn init() -> Option<Self> {
        // Try to use AHCI block device first, fall back to memory
        let block_device: &'static dyn BlockDevice = if let Some(ahci) = AhciBlockDevice::new() {
            crate::serial_write("Using AHCI block device for filesystem\n");
            &ahci
        } else {
            crate::serial_write("Using memory block device for filesystem\n");
            // For now, create an in-memory filesystem
            // In a real implementation, this would read from disk
            Box::leak(Box::new(MemoryBlockDevice::new(1024))) // Small filesystem for demo
        };

        // For now, create an in-memory filesystem
        // In a real implementation, this would read from disk

        // Allocate bitmaps and inode table
        let total_blocks = block_device.total_blocks() as usize;
        let total_inodes = 256;

        // Allocate frames for bitmaps and inodes
        let inode_bitmap_frames = (total_inodes + 4095) / 4096;
        let block_bitmap_frames = (total_blocks + 4095) / 4096;
        let inode_table_frames = (total_inodes * mem::size_of::<Inode>() + 4095) / 4096;

        // For simplicity, use fixed addresses (would use frame allocator in real impl)
        let inode_bitmap_start = PhysAddr::new(0x1000000); // 16MB
        let block_bitmap_start = inode_bitmap_start + (inode_bitmap_frames as u64 * 4096);
        let inode_table_start = block_bitmap_start + (block_bitmap_frames as u64 * 4096);

        unsafe {
            // Initialize bitmaps to all free
            let inode_bitmap_ptr = inode_bitmap_start.as_mut_ptr::<u64>();
            let block_bitmap_ptr = block_bitmap_start.as_mut_ptr::<u64>();
            let inodes_ptr = inode_table_start.as_mut_ptr::<Inode>();

            core::ptr::write_bytes(inode_bitmap_ptr, 0, inode_bitmap_frames * 512);
            core::ptr::write_bytes(block_bitmap_ptr, 0, block_bitmap_frames * 512);

            // Create root inode
            let root_inode = Inode {
                inum: 1,
                file_type: FileType::Directory,
                size: 0,
                permissions: Permissions { read: true, write: true, execute: true },
                uid: 0,
                gid: 0,
                atime: 0,
                mtime: 0,
                ctime: 0,
                blocks: [0; 12],
                indirect_block: 0,
                double_indirect_block: 0,
                triple_indirect_block: 0,
            };

            *inodes_ptr.add(1) = root_inode;

            // Mark root inode as used
            *inode_bitmap_ptr |= 1 << 1;

            let superblock = Superblock {
                magic: 0xDEADBEEF,
                block_size: BLOCK_SIZE as u32,
                total_blocks: total_blocks as u32,
                free_blocks: total_blocks as u32,
                total_inodes: total_inodes as u32,
                free_inodes: (total_inodes - 1) as u32, // Root inode used
                root_inode: 1,
                snapshot_root: 1,
            };

            Some(Filesystem {
                block_device,
                superblock,
                inode_bitmap: core::slice::from_raw_parts_mut(inode_bitmap_ptr, inode_bitmap_frames * 512),
                block_bitmap: core::slice::from_raw_parts_mut(block_bitmap_ptr, block_bitmap_frames * 512),
                inodes: core::slice::from_raw_parts_mut(inodes_ptr, total_inodes),
                current_snapshot: 1,
                next_fd: 3, // 0, 1, 2 reserved for stdin/stdout/stderr
                open_files: [None; 256],
            })
        }
    }

    /// Create a new file
    pub fn create_file(&mut self, parent_inum: InodeNum, name: &str) -> Result<InodeNum, FsError> {
        // Find free inode
        let inum = self.allocate_inode()?;

        // Create inode
        let inode = Inode {
            inum,
            file_type: FileType::Regular,
            size: 0,
            permissions: Permissions { read: true, write: true, execute: false },
            uid: 0,
            gid: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            blocks: [0; 12],
            indirect_block: 0,
            double_indirect_block: 0,
            triple_indirect_block: 0,
        };

        self.inodes[inum as usize] = inode;

        // Add to parent directory
        self.add_dir_entry(parent_inum, name, inum)?;

        Ok(inum)
    }

    /// Write to file
    pub fn write_file(&mut self, inum: InodeNum, offset: usize, data: &[u8]) -> Result<usize, FsError> {
        let inode_idx = inum as usize;

        {
            let inode = &self.inodes[inode_idx];
            if inode.file_type != FileType::Regular {
                return Err(FsError::NotRegularFile);
            }
        }

        let end_pos = offset + data.len();
        if end_pos > self.inodes[inode_idx].size {
            self.inodes[inode_idx].size = end_pos;
        }

        // For simplicity, only handle direct blocks
        let block_index = offset / BLOCK_SIZE;
        let block_offset = offset % BLOCK_SIZE;

        if block_index >= 12 {
            return Err(FsError::FileTooLarge);
        }

        // Check if block needs allocation
        let block_num = if self.inodes[inode_idx].blocks[block_index] == 0 {
            let allocated = self.allocate_block()?;
            self.inodes[inode_idx].blocks[block_index] = allocated;
            allocated
        } else {
            self.inodes[inode_idx].blocks[block_index]
        };

        let block_addr = PhysAddr::new(block_num as u64 * BLOCK_SIZE as u64);

        // Write data
        let write_len = core::cmp::min(data.len(), BLOCK_SIZE - block_offset);
        unsafe {
            let block_ptr = block_addr.as_mut_ptr::<u8>().add(block_offset);
            core::ptr::copy_nonoverlapping(data.as_ptr(), block_ptr, write_len);
        }

        Ok(write_len)
    }

    /// Read from file
    pub fn read_file(&self, inum: InodeNum, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let inode = &self.inodes[inum as usize];

        if inode.file_type != FileType::Regular {
            return Err(FsError::NotRegularFile);
        }

        if offset >= inode.size {
            return Ok(0);
        }

        let read_len = core::cmp::min(buffer.len(), inode.size - offset);

        // For simplicity, only handle direct blocks
        let block_index = offset / BLOCK_SIZE;
        let block_offset = offset % BLOCK_SIZE;

        if block_index >= 12 {
            return Err(FsError::FileTooLarge);
        }

        let block_num = inode.blocks[block_index];
        if block_num == 0 {
            return Ok(0);
        }

        let block_addr = PhysAddr::new(block_num as u64 * BLOCK_SIZE as u64);

        // Read data
        let copy_len = core::cmp::min(read_len, BLOCK_SIZE - block_offset);
        unsafe {
            let block_ptr = block_addr.as_ptr::<u8>().add(block_offset);
            core::ptr::copy_nonoverlapping(block_ptr, buffer.as_mut_ptr(), copy_len);
        }

        Ok(copy_len)
    }

    /// Create snapshot (copy-on-write)
    pub fn create_snapshot(&mut self) -> Result<InodeNum, FsError> {
        // For simplicity, just create a new root inode that shares blocks
        // In a real implementation, this would copy metadata and mark blocks as COW

        let snapshot_inum = self.allocate_inode()?;
        let snapshot_inode = Inode {
            inum: snapshot_inum,
            file_type: FileType::Directory,
            size: self.inodes[self.superblock.root_inode as usize].size,
            permissions: self.inodes[self.superblock.root_inode as usize].permissions,
            uid: 0,
            gid: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            blocks: self.inodes[self.superblock.root_inode as usize].blocks,
            indirect_block: self.inodes[self.superblock.root_inode as usize].indirect_block,
            double_indirect_block: self.inodes[self.superblock.root_inode as usize].double_indirect_block,
            triple_indirect_block: self.inodes[self.superblock.root_inode as usize].triple_indirect_block,
        };

        self.inodes[snapshot_inum as usize] = snapshot_inode;
        self.current_snapshot = snapshot_inum;

        Ok(snapshot_inum)
    }

    /// Allocate a free inode
    fn allocate_inode(&mut self) -> Result<InodeNum, FsError> {
        for i in 1..self.superblock.total_inodes {
            let byte_index = (i / 64) as usize;
            let bit_index = (i % 64) as usize;

            if byte_index < self.inode_bitmap.len() && (self.inode_bitmap[byte_index] & (1 << bit_index)) == 0 {
                self.inode_bitmap[byte_index] |= 1 << bit_index;
                self.superblock.free_inodes -= 1;
                return Ok(i);
            }
        }
        Err(FsError::NoFreeInodes)
    }

    /// Allocate a free block
    fn allocate_block(&mut self) -> Result<BlockNum, FsError> {
        for i in 0..self.superblock.total_blocks {
            let byte_index = (i / 64) as usize;
            let bit_index = (i % 64) as usize;

            if byte_index < self.block_bitmap.len() && (self.block_bitmap[byte_index] & (1 << bit_index)) == 0 {
                self.block_bitmap[byte_index] |= 1 << bit_index;
                self.superblock.free_blocks -= 1;
                return Ok(i);
            }
        }
        Err(FsError::NoFreeBlocks)
    }

    /// Add directory entry
    fn add_dir_entry(&mut self, dir_inum: InodeNum, name: &str, inum: InodeNum) -> Result<(), FsError> {
        // For simplicity, assume directory fits in one block
        let block_num = {
            let dir_inode = &self.inodes[dir_inum as usize];
            if dir_inode.blocks[0] == 0 {
                self.allocate_block()?
            } else {
                dir_inode.blocks[0]
            }
        };

        // Set block if not set
        if self.inodes[dir_inum as usize].blocks[0] == 0 {
            self.inodes[dir_inum as usize].blocks[0] = block_num;
        }

        let block_addr = PhysAddr::new(block_num as u64 * BLOCK_SIZE as u64);
        let dir_entries = unsafe {
            &mut *(block_addr.as_mut_ptr::<[DirEntry; BLOCK_SIZE / mem::size_of::<DirEntry>()]>())
        };

        // Find free slot
        for entry in dir_entries.iter_mut() {
            if entry.name_len == 0 {
                // Copy name
                let name_bytes = name.as_bytes();
                let copy_len = core::cmp::min(name_bytes.len(), MAX_FILENAME_LEN);
                entry.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                entry.name_len = copy_len as u8;
                entry.inum = inum;
                return Ok(());
            }
        }

        Err(FsError::DirectoryFull)
    }

    /// Lookup directory entry by name
    fn lookup_dir_entry(&self, dir_inum: InodeNum, name: &str) -> Result<Option<InodeNum>, FsError> {
        let inode = &self.inodes[dir_inum as usize];
        if inode.file_type != FileType::Directory {
            return Err(FsError::FileNotFound);
        }

        // Read directory data
        let mut buffer = [0u8; BLOCK_SIZE];
        let bytes_read = self.read_inode_data(dir_inum, 0, &mut buffer)?;

        let entries = bytes_read / core::mem::size_of::<DirEntry>();
        for i in 0..entries {
            let offset = i * core::mem::size_of::<DirEntry>();
            let entry: &DirEntry = unsafe {
                &*buffer.as_ptr().add(offset).cast()
            };

            if entry.inum != 0 {
                let entry_name = core::str::from_utf8(&entry.name[..entry.name_len as usize])
                    .map_err(|_| FsError::FileNotFound)?;
                if entry_name == name {
                    return Ok(Some(entry.inum));
                }
            }
        }

        Ok(None)
    }

    /// Read data from inode
    fn read_inode_data(&self, inum: InodeNum, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let inode = &self.inodes[inum as usize];
        let bytes_to_read = core::cmp::min(buffer.len(), inode.size - offset);

        if bytes_to_read == 0 {
            return Ok(0);
        }

        let mut remaining = bytes_to_read;
        let mut buffer_offset = 0;
        let mut file_offset = offset;

        while remaining > 0 {
            let block_index = file_offset / BLOCK_SIZE;
            let block_offset = file_offset % BLOCK_SIZE;
            let bytes_in_block = core::cmp::min(remaining, BLOCK_SIZE - block_offset);

            if let Some(block_num) = self.get_block_num(inum, block_index)? {
                let block_addr = self.get_block_addr(block_num);
                unsafe {
                    let block_data = core::slice::from_raw_parts(
                        block_addr.as_ptr::<u8>(),
                        BLOCK_SIZE
                    );
                    buffer[buffer_offset..buffer_offset + bytes_in_block]
                        .copy_from_slice(&block_data[block_offset..block_offset + bytes_in_block]);
                }
            } else {
                // Sparse block, fill with zeros
                for i in 0..bytes_in_block {
                    buffer[buffer_offset + i] = 0;
                }
            }

            remaining -= bytes_in_block;
            buffer_offset += bytes_in_block;
            file_offset += bytes_in_block;
        }

        Ok(bytes_to_read)
    }

    /// Get block number for inode at given index
    fn get_block_num(&self, inum: InodeNum, block_index: usize) -> Result<Option<BlockNum>, FsError> {
        let inode = &self.inodes[inum as usize];

        if block_index < 12 {
            // Direct block
            let block_num = inode.blocks[block_index];
            Ok(if block_num != 0 { Some(block_num) } else { None })
        } else {
            // Indirect blocks (simplified - not fully implemented)
            Err(FsError::FileTooLarge)
        }
    }

    /// Open a file
    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileDescriptor, FsError> {
        // For now, only support root directory files
        if path.starts_with('/') {
            let filename = &path[1..];
            if filename.is_empty() {
                return Err(FsError::FileNotFound);
            }

            // Look for existing file in root directory
            let root_inum = self.superblock.root_inode;
            if let Some(inum) = self.lookup_dir_entry(root_inum, filename)? {
                // File exists
                if flags.create && flags.truncate {
                    // Truncate existing file
                    self.truncate_file(inum)?;
                }
                let fd = self.allocate_fd()?;
                self.open_files[fd as usize] = Some(OpenFile {
                    inum,
                    position: 0,
                    flags,
                });
                Ok(fd)
            } else if flags.create {
                // Create new file
                let inum = self.create_file(root_inum, filename)?;
                let fd = self.allocate_fd()?;
                self.open_files[fd as usize] = Some(OpenFile {
                    inum,
                    position: 0,
                    flags,
                });
                Ok(fd)
            } else {
                Err(FsError::FileNotFound)
            }
        } else {
            Err(FsError::FileNotFound)
        }
    }

    /// Close a file
    pub fn close(&mut self, fd: FileDescriptor) -> Result<(), FsError> {
        if fd >= self.open_files.len() as FileDescriptor || self.open_files[fd as usize].is_none() {
            return Err(FsError::FileNotFound);
        }
        self.open_files[fd as usize] = None;
        Ok(())
    }

    /// Read from file
    pub fn read(&mut self, fd: FileDescriptor, buffer: &mut [u8]) -> Result<usize, FsError> {
        let (inum, position, flags) = {
            let open_file = self.open_files[fd as usize].as_ref().ok_or(FsError::FileNotFound)?;
            (open_file.inum, open_file.position, open_file.flags)
        };

        if !flags.read {
            return Err(FsError::PermissionDenied);
        }

        let inode = &self.inodes[inum as usize];
        let bytes_to_read = core::cmp::min(buffer.len(), inode.size - position);

        if bytes_to_read == 0 {
            return Ok(0);
        }

        // Read data from blocks
        let mut remaining = bytes_to_read;
        let mut buffer_offset = 0;
        let mut file_offset = position;

        while remaining > 0 {
            let block_index = file_offset / BLOCK_SIZE;
            let block_offset = file_offset % BLOCK_SIZE;
            let bytes_in_block = core::cmp::min(remaining, BLOCK_SIZE - block_offset);

            if let Some(block_num) = self.get_block_num(inum, block_index)? {
                let block_addr = self.get_block_addr(block_num);
                unsafe {
                    let block_data = core::slice::from_raw_parts(
                        block_addr.as_ptr::<u8>(),
                        BLOCK_SIZE
                    );
                    buffer[buffer_offset..buffer_offset + bytes_in_block]
                        .copy_from_slice(&block_data[block_offset..block_offset + bytes_in_block]);
                }
            } else {
                // Sparse block, fill with zeros
                for i in 0..bytes_in_block {
                    buffer[buffer_offset + i] = 0;
                }
            }

            remaining -= bytes_in_block;
            buffer_offset += bytes_in_block;
            file_offset += bytes_in_block;
        }

        self.open_files[fd as usize].as_mut().unwrap().position += bytes_to_read;
        Ok(bytes_to_read)
    }

    /// Write to file
    pub fn write(&mut self, fd: FileDescriptor, data: &[u8]) -> Result<usize, FsError> {
        let (inum, position) = {
            let open_file = self.open_files[fd as usize].as_ref().ok_or(FsError::FileNotFound)?;
            if !open_file.flags.write {
                return Err(FsError::PermissionDenied);
            }
            (open_file.inum, open_file.position)
        };

        let bytes_written = self.write_file(inum, position, data)?;
        self.open_files[fd as usize].as_mut().unwrap().position += bytes_written;
        Ok(bytes_written)
    }

    /// Allocate a file descriptor
    fn allocate_fd(&mut self) -> Result<FileDescriptor, FsError> {
        for i in 0..self.open_files.len() {
            if self.open_files[i].is_none() {
                let fd = self.next_fd;
                self.next_fd += 1;
                return Ok(fd);
            }
        }
        Err(FsError::FileTooLarge) // No more FDs available
    }

    /// Truncate a file to zero size
    fn truncate_file(&mut self, inum: InodeNum) -> Result<(), FsError> {
        let inode = &mut self.inodes[inum as usize];
        inode.size = 0;

        // Collect blocks to free (simplified - only direct blocks)
        let mut blocks_to_free = [0u32; 12];
        let mut count = 0;

        for i in 0..inode.blocks.len() {
            if inode.blocks[i] != 0 {
                blocks_to_free[count] = inode.blocks[i];
                count += 1;
                inode.blocks[i] = 0;
            }
        }

        // Free all blocks
        for i in 0..count {
            self.free_block(blocks_to_free[i])?;
        }

        Ok(())
    }

    /// Get block address
    fn get_block_addr(&self, block_num: BlockNum) -> PhysAddr {
        // Simplified - in real filesystem, this would map block numbers to physical addresses
        PhysAddr::new(0x2000000 + (block_num as u64 * BLOCK_SIZE as u64)) // 32MB + block offset
    }

    /// Free a block
    fn free_block(&mut self, block_num: BlockNum) -> Result<(), FsError> {
        let block_index = block_num as usize;
        let bitmap_index = block_index / 64;
        let bit_index = block_index % 64;

        self.block_bitmap[bitmap_index] &= !(1u64 << bit_index);
        self.superblock.free_blocks += 1;
        Ok(())
    }
}

/// Filesystem errors
#[derive(Debug)]
pub enum FsError {
    NoFreeInodes,
    NoFreeBlocks,
    NotRegularFile,
    FileTooLarge,
    DirectoryFull,
    FileNotFound,
    PermissionDenied,
}

/// Global filesystem instance
static mut FILESYSTEM: Option<Filesystem> = None;

/// Initialize global filesystem
pub fn init() {
    unsafe {
        FILESYSTEM = Filesystem::init();
    }
}

/// Get filesystem instance
#[allow(static_mut_refs)]
pub fn get_fs() -> *mut Filesystem {
    unsafe {
        FILESYSTEM.as_mut().map(|fs| fs as *mut Filesystem).unwrap_or(core::ptr::null_mut())
    }
}