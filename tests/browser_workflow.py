"""Opt-in real Chromium workflow check. Screenshots go to the supplied external directory."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import tempfile
from threading import Thread

from playwright.sync_api import sync_playwright

from examples.browser_fixture import create
from orbit_research.browser import export_browser
from orbit_research.index import rebuild


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def check(destination):
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        config_path = create(root/'owners')
        config = json.loads(config_path.read_text())
        config['checkout_urls']['orrery']='/owners/orrery/'
        config['media'].append(dict(label='Private remote artifact',role='empirical-evidence',record=config['media'][0]['record'],url='https://private.example.invalid/evidence'))
        config_path.write_text(json.dumps(config))
        record = root/'owners/owner-records/00.json'
        r=json.loads(record.read_text()); r['presentation']['note']='</script><script>window.PWNED=true</script><img src="https://private.example.invalid/tracker">'
        record.write_text(json.dumps(r))
        rebuild(config_path,root/'index.sqlite')
        export_browser(root/'index.sqlite',root/'site',config_path=config_path)
        server=ThreadingHTTPServer(('127.0.0.1',0),partial(QuietHandler,directory=str(root)))
        thread=Thread(target=server.serve_forever,daemon=True); thread.start()
        errors, requests, checks = [], [], []
        try:
            with sync_playwright() as p:
                browser=p.chromium.launch()
                page=browser.new_page(viewport={'width':1440,'height':1100},device_scale_factor=1)
                page.on('pageerror',lambda e:errors.append(str(e)))
                page.on('request',lambda r:requests.append(r.url))
                url=f'http://127.0.0.1:{server.server_port}/site/'
                page.goto(url); page.wait_for_selector('#detail h2')
                assert page.locator('#stats').inner_text().count('owners')==1
                page.locator('#search').fill('wide-binary-power')
                assert 'inconclusive' in page.locator('#detail').inner_text()
                assert '33.3%' in page.locator('#detail').inner_text()
                assert page.locator('#detail .tree').get_by_role('button',name='Completed wide-binary control run').count()==1
                checks.append('Claim opens exact assessment and cross-owner protocol/run/data trace')
                page.screenshot(path=str(destination/'desktop.png'),full_page=False)
                page.locator('#detail .tree').get_by_role('button',name='Completed wide-binary control run').click()
                assert 'completed' in page.locator('.axes').inner_text()
                assert 'failed' in page.locator('.axes').inner_text()
                assert 'pending' in page.locator('.callout').inner_text()
                assert page.get_by_role('link',name='Open simulation').count()==1
                assert page.get_by_role('img',name='Synthetic apparatus illustration (illustration)').count()==1
                page.get_by_role('img',name='Synthetic apparatus illustration (illustration)').scroll_into_view_if_needed()
                assert page.get_by_role('img').evaluate('(img)=>img.complete && img.naturalWidth>0')
                with page.expect_popup() as popup:
                    page.get_by_role('link',name='Open simulation').click()
                popup.value.wait_for_load_state(); assert 'Explicit navigation fixture' in popup.value.locator('body').inner_text(); popup.value.close()
                checks.append('Completed failed-control run stays pending; raster loads; simulation requires explicit navigation')
                page.locator('#search').fill('')
                page.locator('#owner').select_option('parallax'); page.locator('#kind').select_option('experiment')
                assert 'completed' in page.locator('.axes').inner_text()
                page.get_by_text('Full source content and legacy qualifications',exact=True).click()
                assert 'Exploratory only; cannot advance H08' in page.locator('#detail').inner_text()
                assert 'preregistered' in page.locator('#detail').inner_text()
                checks.append('Parallax H08/E01 completed exploratory history and raw source qualifications remain visible')
                for width in (390,):
                    page.set_viewport_size({'width':width,'height':844})
                    page.locator('#detail').evaluate('(e)=>e.scrollIntoView({block:"start"})')
                    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
                    page.screenshot(path=str(destination/'narrow.png'),full_page=False)
                    page.evaluate('document.documentElement.style.fontSize="32px"')
                    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
                    page.evaluate('document.documentElement.style.fontSize=""')
                checks.append('390px layout and 200% text have no horizontal overflow')
                page.locator('#search').fill('no-such-record-11392')
                assert page.locator('#detail h2').inner_text()=='No matching records'
                page.goto(url+'#absent-exact-pin')
                assert page.locator('#detail h2').inner_text()=='Snapshot not found'
                checks.append('Empty search and absent pinned navigation never substitute a newer record')
                assert not page.evaluate('Boolean(window.PWNED)')
                assert not any('private.example.invalid' in u for u in requests)
                assert not errors,errors
                page.goto((root/'site/index.html').as_uri()); page.wait_for_selector('#detail h2')
                assert page.locator('#detail h2').inner_text()!=''
                checks.append('Direct file opening works; imported scripts do not execute; zero remote fetches or page errors')
                browser.close()
        finally:
            server.shutdown(); server.server_close(); thread.join()
        result=dict(checks=checks,requests=len(requests),page_errors=errors,viewports=[1440,390],screenshots=['desktop.png','narrow.png'])
        (destination/'browser-results.json').write_text(json.dumps(result,indent=2)+'\n')
        return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output',type=Path)
    print(json.dumps(check(parser.parse_args().output),indent=2))
