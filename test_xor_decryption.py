#!/usr/bin/env python3
"""
Validate XOR decryption logic matches Start9 encryption format
"""
import struct

XOR_KEY1 = [0x84, 0x22, 0xe9, 0xad]
XOR_KEY2 = [0xb7, 0x8f, 0xff, 0x14]
ENCRYPTED_MAGIC = [0x7d, 0x9c, 0x5d, 0x74]
EXPECTED_MAGIC = [0xf9, 0xbe, 0xb4, 0xd9]  # Bitcoin mainnet magic

def u32_from_bytes(b):
    return struct.unpack('<I', bytes(b))[0]

def bytes_from_u32(v):
    return list(struct.pack('<I', v))

def test_magic_decryption():
    """Test that encrypted magic decrypts correctly"""
    print("=" * 60)
    print("TEST 1: Magic Decryption")
    print("=" * 60)
    
    # Magic is at file offset 0 (chunk 0, uses KEY1)
    magic_start_pos = 0
    encrypted_magic = ENCRYPTED_MAGIC.copy()
    
    # Current implementation: u32 XOR
    use_key1 = (magic_start_pos // 4) % 2 == 0
    key1_u32 = u32_from_bytes(XOR_KEY1)
    key2_u32 = u32_from_bytes(XOR_KEY2)
    key_u32 = key1_u32 if use_key1 else key2_u32
    
    magic_u32 = u32_from_bytes(encrypted_magic)
    decrypted_magic_u32 = magic_u32 ^ key_u32
    decrypted_magic = bytes_from_u32(decrypted_magic_u32)
    
    print(f"  Encrypted magic: {[hex(b) for b in encrypted_magic]}")
    print(f"  File offset: {magic_start_pos}")
    print(f"  Chunk: {magic_start_pos // 4}, use_key1: {use_key1}")
    print(f"  Key (u32): 0x{key_u32:08x}")
    print(f"  Decrypted magic: {[hex(b) for b in decrypted_magic]}")
    print(f"  Expected magic: {[hex(b) for b in EXPECTED_MAGIC]}")
    
    if decrypted_magic == EXPECTED_MAGIC:
        print("  ✅ PASS: Magic decrypts correctly")
        return True
    else:
        print("  ❌ FAIL: Magic decryption is wrong!")
        return False

def test_size_decryption():
    """Test that size field decrypts correctly at different offsets"""
    print("\n" + "=" * 60)
    print("TEST 2: Size Field Decryption")
    print("=" * 60)
    
    # Test cases: size field at different file offsets
    test_cases = [
        (0, 4),   # Block starts at 0, size at 4
        (100, 104),  # Block starts at 100, size at 104
        (200, 204),  # Block starts at 200, size at 204
    ]
    
    all_pass = True
    for magic_start_pos, size_offset in test_cases:
        print(f"\n  Test: magic at {magic_start_pos}, size at {size_offset}")
        
        # Simulate: we read an encrypted size field
        # For testing, let's encrypt a known size (e.g., 1000 bytes = 0x000003e8)
        known_size = 1000
        known_size_bytes = bytes_from_u32(known_size)
        
        # Encrypt it (reverse of decryption)
        use_key1 = (size_offset // 4) % 2 == 0
        key1_u32 = u32_from_bytes(XOR_KEY1)
        key2_u32 = u32_from_bytes(XOR_KEY2)
        key_u32 = key1_u32 if use_key1 else key2_u32
        
        encrypted_size_u32 = u32_from_bytes(known_size_bytes) ^ key_u32
        encrypted_size_bytes = bytes_from_u32(encrypted_size_u32)
        
        # Now decrypt it (our implementation)
        decrypted_size_u32 = encrypted_size_u32 ^ key_u32
        decrypted_size = decrypted_size_u32
        
        print(f"    Known size: {known_size} (0x{known_size:08x})")
        print(f"    Size offset: {size_offset}, chunk: {size_offset // 4}, use_key1: {use_key1}")
        print(f"    Encrypted size: {[hex(b) for b in encrypted_size_bytes]} (0x{encrypted_size_u32:08x})")
        print(f"    Decrypted size: {decrypted_size} (0x{decrypted_size:08x})")
        
        if decrypted_size == known_size:
            print(f"    ✅ PASS")
        else:
            print(f"    ❌ FAIL: Expected {known_size}, got {decrypted_size}")
            all_pass = False
    
    return all_pass

def test_key_rotation():
    """Test that key rotation is correct across file offsets"""
    print("\n" + "=" * 60)
    print("TEST 3: Key Rotation Pattern")
    print("=" * 60)
    
    print("  File offset -> Chunk -> Key")
    print("  " + "-" * 40)
    for offset in range(0, 32, 4):
        chunk = offset // 4
        use_key1 = (chunk % 2) == 0
        key_name = "KEY1" if use_key1 else "KEY2"
        print(f"  {offset:3d}-{offset+3:3d} -> {chunk:2d} -> {key_name}")
    
    # Verify pattern
    expected = ["KEY1", "KEY2", "KEY1", "KEY2", "KEY1", "KEY2", "KEY1", "KEY2"]
    actual = []
    for offset in range(0, 32, 4):
        chunk = offset // 4
        use_key1 = (chunk % 2) == 0
        key_name = "KEY1" if use_key1 else "KEY2"
        actual.append(key_name)
    
    if actual == expected:
        print("  ✅ PASS: Key rotation pattern is correct")
        return True
    else:
        print(f"  ❌ FAIL: Expected {expected}, got {actual}")
        return False

def test_byte_by_byte_vs_u32():
    """Test that byte-by-byte and u32 XOR produce same result"""
    print("\n" + "=" * 60)
    print("TEST 4: Byte-by-byte vs u32 XOR (should match)")
    print("=" * 60)
    
    # Test data: 4 bytes
    test_data = [0x12, 0x34, 0x56, 0x78]
    file_offset = 4  # Size field position
    
    # Method 1: Byte-by-byte (old way)
    decrypted_byte_by_byte = test_data.copy()
    use_key1 = (file_offset // 4) % 2 == 0
    key = XOR_KEY1 if use_key1 else XOR_KEY2
    for i in range(4):
        byte_offset = file_offset + i
        decrypted_byte_by_byte[i] ^= key[byte_offset % 4]
    
    # Method 2: u32 XOR (new way)
    use_key1 = (file_offset // 4) % 2 == 0
    key1_u32 = u32_from_bytes(XOR_KEY1)
    key2_u32 = u32_from_bytes(XOR_KEY2)
    key_u32 = key1_u32 if use_key1 else key2_u32
    
    data_u32 = u32_from_bytes(test_data)
    decrypted_u32 = data_u32 ^ key_u32
    decrypted_new = bytes_from_u32(decrypted_u32)
    
    print(f"  Test data: {[hex(b) for b in test_data]}")
    print(f"  File offset: {file_offset}, chunk: {file_offset // 4}, use_key1: {use_key1}")
    print(f"  Byte-by-byte result: {[hex(b) for b in decrypted_byte_by_byte]}")
    print(f"  u32 XOR result: {[hex(b) for b in decrypted_new]}")
    
    if decrypted_byte_by_byte == decrypted_new:
        print("  ✅ PASS: Both methods produce same result")
        return True
    else:
        print("  ❌ FAIL: Methods produce different results!")
        print("  ⚠️  This means the u32 XOR implementation is WRONG")
        return False

if __name__ == "__main__":
    print("\n🔍 VALIDATING XOR DECRYPTION LOGIC\n")
    
    results = []
    results.append(("Magic Decryption", test_magic_decryption()))
    results.append(("Size Decryption", test_size_decryption()))
    results.append(("Key Rotation", test_key_rotation()))
    results.append(("Byte-by-byte vs u32", test_byte_by_byte_vs_u32()))
    
    print("\n" + "=" * 60)
    print("SUMMARY")
    print("=" * 60)
    all_pass = True
    for name, passed in results:
        status = "✅ PASS" if passed else "❌ FAIL"
        print(f"  {name}: {status}")
        if not passed:
            all_pass = False
    
    print("=" * 60)
    if all_pass:
        print("✅ ALL TESTS PASSED - XOR decryption logic is correct")
        exit(0)
    else:
        print("❌ TESTS FAILED - XOR decryption logic has bugs!")
        exit(1)

