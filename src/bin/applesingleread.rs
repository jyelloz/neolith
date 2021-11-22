use std::{
    env,
    fs,
    io::{self, Read as _, Write as _},
};

use neolith::apple::AppleSingleHeader;

fn main() -> io::Result<()> {
    let file: String = env::args()
        .skip(1)
        .take(1)
        .next()
        .expect("no args");
    let rsrc = format!("{}.rsrc", &file);
    let data = format!("{}.data", &file);

    let mut applesingle = fs::File::open(&file)?;
    let applesingle_md = applesingle.metadata()?;

    let applesingle_data = {
        let mut data = Vec::with_capacity(applesingle_md.len() as usize);
        applesingle.read_to_end(&mut data)?;
        data
    };
    let (_, header) = AppleSingleHeader::from_bytes(&applesingle_data)
        .expect("failed to parse applesingle header");

    let data_fork = header.data_fork()
        .expect("no data fork");
    let rsrc_fork = header.resource_fork()
        .expect("no resource fork");

    let data_offset = data_fork.offset as usize;
    let rsrc_offset = rsrc_fork.offset as usize;

    let data_bytes = &applesingle_data[data_offset..][..data_fork.length as usize];
    let rsrc_bytes = &applesingle_data[rsrc_offset..][..rsrc_fork.length as usize];

    let mut rsrc = std::fs::File::create(&rsrc)?;
    rsrc.write_all(rsrc_bytes)?;

    let mut data = std::fs::File::create(&data)?;
    data.write_all(data_bytes)?;

    Ok(())
}
