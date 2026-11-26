"""Check the facade about to be loaded by an ABI test in its loader environment."""
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.cell import CellError, Settings
from tools.dev.drivers import DriverServices


if __name__ == '__main__':
    try:
        services = DriverServices(Settings.read(os.environ))
        services.check_object(Path(sys.argv[1]), runner='ABI ownership', side='subject',
                              cwd=Path.cwd(), environment=os.environ)
    except CellError as error:
        sys.exit(str(error))
