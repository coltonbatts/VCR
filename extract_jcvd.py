import re
import json
import sys

def extract_frames(html_path, out_path):
    with open(html_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # regex for n[0] = '...';
    # it can span multiple lines and contains \n
    pattern = r"n\[\d+\] = '(.*?)';"
    matches = re.findall(pattern, content, re.DOTALL)
    
    print(f"Found {len(matches)} frames.")
    
    # Unescape the strings
    frames = [m.encode().decode('unicode_escape') for m in matches]
    
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(frames, f, indent=2)

if __name__ == "__main__":
    extract_frames('jcvd_source_3.html', 'jcvd_frames.json')
